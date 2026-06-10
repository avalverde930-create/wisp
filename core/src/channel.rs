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
}
