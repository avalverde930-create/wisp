//! core::transport — the native data plane (quinn/QUIC): encryption, congestion
//! control, datagrams, connection migration (survives Wi-Fi<->cellular). webrtc-rs
//! is added in Phase 3 (browser + extra NAT) BEHIND a MediaTransport trait extracted
//! then. The packet decode path here is a designated cargo-fuzz target. Never custom UDP.
//!
//! ============================ PHASE-0a SPIKE SECURITY NOTE ============================
//! QUIC mandates TLS. For the LAN spike the client SKIPS server-certificate
//! verification (`SkipServerVerification`) and the server uses a throwaway
//! self-signed cert. This is NOT the product's security model. In Phase 1 the real
//! authentication + confidentiality is the Noise XX/IK channel + out-of-band SAS
//! pairing (ADR-0003), and every frame rides INSIDE that envelope. This module
//! exists to prove the capture->transport->render->input loop, not to secure it.
//! =====================================================================================

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use quinn::{ClientConfig, Connection, Endpoint, RecvStream, SendStream, ServerConfig};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};

use crate::wire::{FrameHeader, InputEvent, WireError};

/// ALPN id for the spike protocol.
pub const ALPN: &[u8] = b"wisp/0";

fn ring_provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// Build a QUIC **server** endpoint bound to `bind`, using a throwaway self-signed cert.
pub fn server_endpoint(bind: SocketAddr) -> Result<Endpoint> {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
        .context("generate self-signed cert")?;
    let cert_der: CertificateDer<'static> = cert.cert.der().clone();
    let key_der: PrivateKeyDer<'static> =
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der()));

    let mut tls = rustls::ServerConfig::builder_with_provider(ring_provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .context("tls13 server config")?
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key_der)
        .context("with_single_cert")?;
    tls.alpn_protocols = vec![ALPN.to_vec()];

    let server_config = ServerConfig::with_crypto(Arc::new(
        QuicServerConfig::try_from(tls).context("quic server config")?,
    ));
    Endpoint::server(server_config, bind).context("bind server endpoint")
}

/// Build a QUIC **client** endpoint (ephemeral port). SPIKE ONLY: skips cert verification.
pub fn client_endpoint() -> Result<Endpoint> {
    let provider = ring_provider();
    let verifier = Arc::new(SkipServerVerification(provider.clone()));

    let mut tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .context("tls13 client config")?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    tls.alpn_protocols = vec![ALPN.to_vec()];

    let client_config = ClientConfig::new(Arc::new(
        QuicClientConfig::try_from(tls).context("quic client config")?,
    ));

    let mut endpoint =
        Endpoint::client("0.0.0.0:0".parse().unwrap()).context("bind client endpoint")?;
    endpoint.set_default_client_config(client_config);
    Ok(endpoint)
}

/// Dial a host. `server_name` is cosmetic for the spike (verification is skipped).
pub async fn connect(endpoint: &Endpoint, addr: SocketAddr) -> Result<Connection> {
    endpoint
        .connect(addr, "localhost")
        .context("start connect")?
        .await
        .context("quic handshake")
}

// ---------------------------------------------------------------------------
// Media-frame stream helpers (host -> client, ordered uni stream)
// ---------------------------------------------------------------------------

pub async fn write_frame(
    send: &mut SendStream,
    header: &FrameHeader,
    payload: &[u8],
) -> Result<()> {
    send.write_all(&header.encode())
        .await
        .context("write frame header")?;
    send.write_all(payload)
        .await
        .context("write frame payload")?;
    Ok(())
}

pub async fn read_frame(recv: &mut RecvStream) -> Result<(FrameHeader, Vec<u8>)> {
    let mut hbuf = [0u8; FrameHeader::ENCODED_LEN];
    recv.read_exact(&mut hbuf)
        .await
        .context("read frame header")?;
    let header = FrameHeader::decode(&hbuf).map_err(wire_err)?;
    let mut payload = vec![0u8; header.payload_len as usize];
    recv.read_exact(&mut payload)
        .await
        .context("read frame payload")?;
    Ok((header, payload))
}

// ---------------------------------------------------------------------------
// Input-event stream helpers (client -> host, reliable uni stream)
// ---------------------------------------------------------------------------

pub async fn write_input(send: &mut SendStream, ev: &InputEvent) -> Result<()> {
    let body = ev.encode();
    send.write_all(&(body.len() as u16).to_be_bytes())
        .await
        .context("write input len")?;
    send.write_all(&body).await.context("write input body")?;
    Ok(())
}

pub async fn read_input(recv: &mut RecvStream) -> Result<InputEvent> {
    let mut lb = [0u8; 2];
    recv.read_exact(&mut lb).await.context("read input len")?;
    let len = u16::from_be_bytes(lb) as usize;
    let mut body = vec![0u8; len];
    recv.read_exact(&mut body)
        .await
        .context("read input body")?;
    let (ev, _) = InputEvent::decode(&body).map_err(wire_err)?;
    Ok(ev)
}

// ---------------------------------------------------------------------------
// SPIKE auth handshake: a pre-shared token sent on the control stream BEFORE any
// input is accepted or any frame is sent. NOTE: it crosses the cert-unverified
// channel in cleartext, so it guards against casual/opportunistic LAN access,
// NOT an active MITM. Real authentication is the Phase-1 Noise + SAS pairing.
// ---------------------------------------------------------------------------

pub async fn write_hello(send: &mut SendStream, token: &str) -> Result<()> {
    let body = token.as_bytes();
    send.write_all(&(body.len() as u16).to_be_bytes())
        .await
        .context("write hello len")?;
    send.write_all(body).await.context("write hello body")?;
    Ok(())
}

pub async fn read_hello(recv: &mut RecvStream) -> Result<String> {
    let mut lb = [0u8; 2];
    recv.read_exact(&mut lb).await.context("read hello len")?;
    let len = u16::from_be_bytes(lb) as usize;
    anyhow::ensure!(len <= 256, "hello token too long: {len}");
    let mut body = vec![0u8; len];
    recv.read_exact(&mut body)
        .await
        .context("read hello body")?;
    String::from_utf8(body).context("hello token utf8")
}

fn wire_err(e: WireError) -> anyhow::Error {
    anyhow::anyhow!("wire decode: {e}")
}

// ---------------------------------------------------------------------------
// SPIKE-ONLY: accept any server certificate. Replaced by Noise+SAS in Phase 1.
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct SkipServerVerification(Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}
