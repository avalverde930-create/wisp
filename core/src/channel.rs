//! wisp-core::channel — establish a Noise-secured session over a quinn bi-stream.
//!
//! Drives the Noise XX handshake (`crypto`) by exchanging length-prefixed messages
//! (`framing::write_msg`/`read_msg`) over a bi-stream, and returns the post-handshake
//! AEAD `Session` plus the peer static (cache it for an IK reconnect) and the SAS to
//! compare out-of-band (ADR-0003). This is the integration primitive that replaces the
//! spike's cert-skip + cleartext token in the live host/client — that wiring is next.

use anyhow::{Context, Result};
use quinn::{RecvStream, SendStream};

use crate::crypto::{sas_code, Handshake, Session};
use crate::framing::{read_msg, write_msg};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{crypto, framing, transport};
    use std::time::Duration;
    use tokio::time::timeout;

    /// Real loopback QUIC: run the Noise XX handshake over a bi-stream, confirm a matching
    /// SAS + mutual static auth, then send one Noise-encrypted message end to end.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn noise_xx_over_quinn_loopback() {
        let ini_kp = crypto::generate_static_keypair().unwrap();
        let res_kp = crypto::generate_static_keypair().unwrap();
        let ini_priv = ini_kp.private.clone();
        let res_priv = res_kp.private.clone();

        let server = transport::server_endpoint("127.0.0.1:0".parse().unwrap()).unwrap();
        let addr = server.local_addr().unwrap();
        let client = transport::client_endpoint().unwrap();

        let body = async {
            // Establish the connection on both ends concurrently; keep both alive in scope.
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
                let ct = framing::read_msg(&mut r).await.unwrap();
                let mut sess = est.session;
                let pt = sess.decrypt(&ct).unwrap();
                (pt, est.sas, est.remote_static)
            };
            let client_side = async {
                let (mut s, mut r) = client_conn.open_bi().await.unwrap();
                let mut est = handshake_xx_initiator(&mut s, &mut r, &ini_priv)
                    .await
                    .unwrap();
                let ct = est.session.encrypt(b"secure hello over quic").unwrap();
                framing::write_msg(&mut s, &ct).await.unwrap();
                let _ = s.finish();
                (est.sas, est.remote_static)
            };
            let ((pt, sas_s, server_saw), (sas_c, client_saw)) =
                tokio::join!(server_side, client_side);

            assert_eq!(pt, b"secure hello over quic"); // E2E decrypt works
            assert_eq!(sas_s, sas_c); // both sides derive the same SAS
            assert_eq!(client_saw, res_kp.public); // initiator learned responder static
            assert_eq!(server_saw, ini_kp.public); // responder learned initiator static
        };

        timeout(Duration::from_secs(15), body)
            .await
            .expect("loopback Noise-over-QUIC test timed out");
    }
}
