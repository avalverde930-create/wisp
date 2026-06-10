//! wisp-core::channel — establish a Noise-secured session over a quinn bi-stream, plus
//! chunked encrypted message I/O.
//!
//! `handshake_xx_{initiator,responder}` drive the Noise XX handshake (`crypto`) by
//! exchanging length-prefixed messages (`framing`) over a bi-stream, returning the AEAD
//! `Session` + the peer static (cache it for IK reconnect) + the out-of-band SAS.
//!
//! `write_secure`/`read_secure` carry an arbitrarily-large plaintext as a sequence of
//! Noise records (a Noise transport message is capped at 65535 bytes), so a full frame
//! is split into chunks. This is the integration primitive the live host/client use to
//! replace the spike's cert-skip + cleartext token.

use std::sync::Mutex;

use anyhow::{Context, Result};
use quinn::{RecvStream, SendStream};

use crate::crypto::{sas_code, Handshake, Session};
use crate::framing::{read_msg, write_msg};
use crate::wire::MAX_FRAME_PAYLOAD;

/// A completed secure channel.
pub struct Established {
    /// Post-handshake AEAD session.
    pub session: Session,
    /// The peer's static public key — cache it to enable an IK 0-RTT reconnect.
    pub remote_static: Vec<u8>,
    /// 6-digit SAS the owner compares out-of-band to defeat a pairing-time MITM.
    pub sas: String,
}

/// Initiator side of the XX handshake over a bi-stream (send, recv, send).
pub async fn handshake_xx_initiator(
    send: &mut SendStream,
    recv: &mut RecvStream,
    static_private: &[u8],
) -> Result<Established> {
    let mut hs = Handshake::initiator(static_private)?;
    write_msg(send, &hs.write()?).await?; // -> e
    hs.read(&read_msg(recv).await?)?; // <- e ee s es
    write_msg(send, &hs.write()?).await?; // -> s se
    finish(hs)
}

/// Responder side of the XX handshake over a bi-stream (recv, send, recv).
pub async fn handshake_xx_responder(
    send: &mut SendStream,
    recv: &mut RecvStream,
    static_private: &[u8],
) -> Result<Established> {
    let mut hs = Handshake::responder(static_private)?;
    hs.read(&read_msg(recv).await?)?; // -> e
    write_msg(send, &hs.write()?).await?; // <- e ee s es
    hs.read(&read_msg(recv).await?)?; // -> s se
    finish(hs)
}

fn finish(hs: Handshake) -> Result<Established> {
    let remote_static = hs
        .remote_static()
        .context("handshake produced no remote static")?;
    let sas = sas_code(&hs.handshake_hash());
    let session = hs.into_session()?;
    Ok(Established {
        session,
        remote_static,
        sas,
    })
}

// ---------------------------------------------------------------------------
// Pattern negotiation. The initiator writes a single tag byte first so the responder
// (which cannot tell XX from IK by message shape) knows which handshake to run:
// `HS_XX` = first contact (both statics exchanged, SAS compared out-of-band),
// `HS_IK` = 0-RTT reconnect (the initiator already cached the responder static). ADR-0003.
// ---------------------------------------------------------------------------

/// Pattern tag: Noise XX first-contact.
pub const HS_XX: u8 = 0x00;
/// Pattern tag: Noise IK 0-RTT reconnect (cached responder static).
pub const HS_IK: u8 = 0x01;

/// Initiator side of the IK reconnect over a bi-stream (send, recv): a 2-message 0-RTT
/// handshake that authenticates to the cached responder static.
pub async fn handshake_ik_initiator(
    send: &mut SendStream,
    recv: &mut RecvStream,
    static_private: &[u8],
    remote_static_public: &[u8],
) -> Result<Established> {
    let mut hs = Handshake::initiator_ik(static_private, remote_static_public)?;
    write_msg(send, &hs.write()?).await?; // -> e es s ss
    hs.read(&read_msg(recv).await?)?; // <- e ee se
    finish(hs)
}

/// Responder side of the IK reconnect over a bi-stream (recv, send): learns the initiator
/// static from message 1, so the caller can still apply key pinning afterwards.
pub async fn handshake_ik_responder(
    send: &mut SendStream,
    recv: &mut RecvStream,
    static_private: &[u8],
) -> Result<Established> {
    let mut hs = Handshake::responder_ik(static_private)?;
    hs.read(&read_msg(recv).await?)?; // -> e es s ss
    write_msg(send, &hs.write()?).await?; // <- e ee se
    finish(hs)
}

/// Initiator: write the XX tag, then run the XX first-contact handshake.
pub async fn initiate_xx(
    send: &mut SendStream,
    recv: &mut RecvStream,
    static_private: &[u8],
) -> Result<Established> {
    send.write_all(&[HS_XX]).await.context("write XX tag")?;
    handshake_xx_initiator(send, recv, static_private).await
}

/// Initiator: write the IK tag, then run the IK 0-RTT reconnect with the cached static.
pub async fn initiate_ik(
    send: &mut SendStream,
    recv: &mut RecvStream,
    static_private: &[u8],
    remote_static_public: &[u8],
) -> Result<Established> {
    send.write_all(&[HS_IK]).await.context("write IK tag")?;
    handshake_ik_initiator(send, recv, static_private, remote_static_public).await
}

/// Responder: read the 1-byte pattern tag and run the matching handshake. Returns the
/// negotiated pattern (`HS_XX` / `HS_IK`) alongside the established session.
pub async fn accept(
    send: &mut SendStream,
    recv: &mut RecvStream,
    static_private: &[u8],
) -> Result<(u8, Established)> {
    let mut tag = [0u8; 1];
    recv.read_exact(&mut tag)
        .await
        .context("read handshake pattern tag")?;
    let est = match tag[0] {
        HS_XX => handshake_xx_responder(send, recv, static_private).await?,
        HS_IK => handshake_ik_responder(send, recv, static_private).await?,
        other => anyhow::bail!("unknown handshake pattern tag {other:#x}"),
    };
    Ok((tag[0], est))
}

// ---------------------------------------------------------------------------
// Chunked secure I/O. A Noise transport message is capped at 65535 bytes, so a large
// plaintext (a full frame) is split into records: `write_secure` sends a u32-length
// header record then the data records; `read_secure` reassembles them. The session is
// behind a `Mutex` (shared by the two directional tasks); it is locked only across the
// synchronous AEAD call, never across an await.
// ---------------------------------------------------------------------------

/// Max plaintext per Noise record (Noise caps a message at 65535 bytes incl. the 16B tag).
pub const NOISE_MAX_PLAINTEXT: usize = 65535 - 16;

pub async fn write_secure(
    send: &mut SendStream,
    session: &Mutex<Session>,
    plaintext: &[u8],
) -> Result<()> {
    let len_ct = {
        let mut s = session.lock().unwrap();
        s.encrypt(&(plaintext.len() as u32).to_be_bytes())?
    };
    write_msg(send, &len_ct).await?;
    for chunk in plaintext.chunks(NOISE_MAX_PLAINTEXT) {
        let ct = {
            let mut s = session.lock().unwrap();
            s.encrypt(chunk)?
        };
        write_msg(send, &ct).await?;
    }
    Ok(())
}

pub async fn read_secure(recv: &mut RecvStream, session: &Mutex<Session>) -> Result<Vec<u8>> {
    let len_ct = read_msg(recv).await?;
    let len_pt = {
        let mut s = session.lock().unwrap();
        s.decrypt(&len_ct)?
    };
    anyhow::ensure!(len_pt.len() == 4, "bad secure length header");
    let total = u32::from_be_bytes([len_pt[0], len_pt[1], len_pt[2], len_pt[3]]) as usize;
    anyhow::ensure!(
        total <= MAX_FRAME_PAYLOAD as usize + 1024,
        "secure message too large: {total}"
    );
    let mut out = Vec::with_capacity(total);
    while out.len() < total {
        let ct = read_msg(recv).await?;
        let pt = {
            let mut s = session.lock().unwrap();
            s.decrypt(&ct)?
        };
        out.extend_from_slice(&pt);
        anyhow::ensure!(out.len() <= total, "secure message overran declared length");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{crypto, transport};
    use std::time::Duration;
    use tokio::time::timeout;

    /// Real loopback QUIC: run the Noise XX handshake over a bi-stream, confirm a matching
    /// SAS + mutual static auth, then send a LARGE message (forces multi-record chunking)
    /// via write_secure and reassemble it with read_secure.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn noise_secure_chunked_over_quinn_loopback() {
        let ini_kp = crypto::generate_static_keypair().unwrap();
        let res_kp = crypto::generate_static_keypair().unwrap();
        let ini_priv = ini_kp.private.clone();
        let res_priv = res_kp.private.clone();

        // ~200 KB => 4 Noise records of <=65519 bytes each.
        let big: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();

        let server = transport::server_endpoint("127.0.0.1:0".parse().unwrap()).unwrap();
        let addr = server.local_addr().unwrap();
        let client = transport::client_endpoint().unwrap();

        let body = async {
            let (server_conn, client_conn) = tokio::join!(
                async { server.accept().await.unwrap().await.unwrap() },
                transport::connect(&client, addr),
            );
            let client_conn = client_conn.unwrap();

            let server_side = async {
                let (mut s, mut r) = server_conn.accept_bi().await.unwrap();
                let est = handshake_xx_responder(&mut s, &mut r, &res_priv)
                    .await
                    .unwrap();
                let sess = Mutex::new(est.session);
                let pt = read_secure(&mut r, &sess).await.unwrap();
                (pt, est.sas, est.remote_static)
            };
            let client_side = async {
                let (mut s, mut r) = client_conn.open_bi().await.unwrap();
                let est = handshake_xx_initiator(&mut s, &mut r, &ini_priv)
                    .await
                    .unwrap();
                let sess = Mutex::new(est.session);
                write_secure(&mut s, &sess, &big).await.unwrap();
                let _ = s.finish();
                (est.sas, est.remote_static)
            };
            let ((pt, sas_s, server_saw), (sas_c, client_saw)) =
                tokio::join!(server_side, client_side);

            assert_eq!(pt, big); // large chunked message reassembles exactly
            assert_eq!(sas_s, sas_c); // both sides derive the same SAS
            assert_eq!(client_saw, res_kp.public); // initiator learned responder static
            assert_eq!(server_saw, ini_kp.public); // responder learned initiator static
        };

        timeout(Duration::from_secs(15), body)
            .await
            .expect("loopback Noise-over-QUIC test timed out");
    }

    /// Tagged negotiation over real loopback QUIC: the initiator advertises IK (cached
    /// responder static), `accept` reads the tag and runs the IK responder, and a secure
    /// message round-trips. Confirms the 0-RTT reconnect path end-to-end.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ik_reconnect_negotiated_over_quinn_loopback() {
        let ini_kp = crypto::generate_static_keypair().unwrap();
        let res_kp = crypto::generate_static_keypair().unwrap();
        let ini_priv = ini_kp.private.clone();
        let res_priv = res_kp.private.clone();
        let res_pub = res_kp.public.clone(); // the initiator has this cached from a prior XX

        let server = transport::server_endpoint("127.0.0.1:0".parse().unwrap()).unwrap();
        let addr = server.local_addr().unwrap();
        let client = transport::client_endpoint().unwrap();

        let body = async {
            let (server_conn, client_conn) = tokio::join!(
                async { server.accept().await.unwrap().await.unwrap() },
                transport::connect(&client, addr),
            );
            let client_conn = client_conn.unwrap();

            let server_side = async {
                let (mut s, mut r) = server_conn.accept_bi().await.unwrap();
                let (pattern, est) = accept(&mut s, &mut r, &res_priv).await.unwrap();
                let sess = Mutex::new(est.session);
                let pt = read_secure(&mut r, &sess).await.unwrap();
                (pattern, pt, est.remote_static)
            };
            let client_side = async {
                let (mut s, mut r) = client_conn.open_bi().await.unwrap();
                let est = initiate_ik(&mut s, &mut r, &ini_priv, &res_pub)
                    .await
                    .unwrap();
                let sess = Mutex::new(est.session);
                write_secure(&mut s, &sess, b"reconnect payload")
                    .await
                    .unwrap();
                let _ = s.finish();
                est.remote_static
            };
            let ((pattern, pt, server_saw), client_saw) = tokio::join!(server_side, client_side);

            assert_eq!(pattern, HS_IK); // responder negotiated the IK reconnect
            assert_eq!(pt, b"reconnect payload");
            assert_eq!(server_saw, ini_kp.public); // responder learned initiator static
            assert_eq!(client_saw, res_kp.public); // initiator authenticated the cached static
        };

        timeout(Duration::from_secs(15), body)
            .await
            .expect("loopback IK-over-QUIC test timed out");
    }
}
