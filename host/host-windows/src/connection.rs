//! host-windows::connection — one Noise-secured, pinned client session.
//!
//! The client opens a bi-stream; we run the Noise XX handshake (responder) to get an
//! E2E session + the out-of-band SAS + the client's static. We then apply key PINNING
//! (ADR-0003): a non-loopback client whose static is not pinned is rejected unless the
//! host is in pair mode. The client's first secure message is the access token. After
//! that the bi-stream carries everything encrypted (chunked Noise records): inbound input
//! (decrypt -> inject), outbound captured frames (encrypt -> send).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use wisp_core::channel;
use wisp_core::trust::{self, TrustStore};
use wisp_core::wire::{FrameHeader, InputEvent};

use crate::capture::{capture_loop, CapturedFrame};
use crate::inject::inject;

pub async fn handle_connection(
    conn: quinn::Connection,
    expected_token: Option<String>,
    device_private: Arc<Vec<u8>>,
    trust: Arc<Mutex<TrustStore>>,
    pair_mode: bool,
) -> Result<()> {
    let peer = conn.remote_address();

    // The client opens the bi-stream; we are the Noise XX responder. The handshake gives
    // mutual auth + the SAS + the client's static, and the session encrypts what follows.
    let (mut send, mut recv) =
        match tokio::time::timeout(Duration::from_secs(5), conn.accept_bi()).await {
            Ok(Ok(x)) => x,
            Ok(Err(e)) => anyhow::bail!("accept stream from {peer}: {e}"),
            Err(_) => anyhow::bail!("client {peer} opened no stream within 5s"),
        };
    let (pattern, est) = channel::accept(&mut send, &mut recv, &device_private)
        .await
        .with_context(|| format!("noise handshake with {peer}"))?;
    let fp = trust::fingerprint(&est.remote_static);
    if pattern == channel::HS_IK {
        println!("[host] IK 0-RTT reconnect from {peer} (device {fp})");
    } else {
        println!(
            "[host] XX first-contact from {peer}; pairing SAS: {} (compare with the client)",
            est.sas
        );
    }

    // Key pinning (ADR-0003): loopback is the local trust boundary; otherwise the client
    // static must be pinned, or the host must be in pair mode (then we pin it).
    if peer.ip().is_loopback() {
        println!("[host] loopback device {fp} (trusted)");
    } else if trust.lock().unwrap().is_trusted(&est.remote_static) {
        println!("[host] known device {fp} (pinned)");
    } else if pair_mode {
        trust
            .lock()
            .unwrap()
            .pin(&est.remote_static)
            .context("pin new client")?;
        println!("[host] PAIRED new device {fp} - verify the SAS matches the client");
    } else {
        eprintln!("[host] REJECTED {peer}: unknown device {fp}. Re-run host with WISP_PAIR=1 to pair (and verify the SAS).");
        conn.close(2u32.into(), b"unknown device");
        return Ok(());
    }

    // First secure message = the access token (the spike LAN guardrail).
    let session = Arc::new(Mutex::new(est.session));
    let token_pt = channel::read_secure(&mut recv, &session)
        .await
        .context("read token")?;
    let token = String::from_utf8(token_pt).context("token utf8")?;
    if let Some(expected) = &expected_token {
        if &token != expected {
            eprintln!("[host] REJECTED {peer}: bad token");
            conn.close(1u32.into(), b"bad token");
            return Ok(());
        }
    }
    println!("[host] client authenticated: {peer}");

    // input: decrypt inbound messages -> InputEvent -> inject.
    let in_session = session.clone();
    tokio::spawn(async move {
        loop {
            let pt = match channel::read_secure(&mut recv, &in_session).await {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("[host] input stream ended: {e}");
                    break;
                }
            };
            match InputEvent::decode(&pt) {
                Ok((ev, _)) => inject(ev),
                Err(e) => {
                    eprintln!("[host] input decode failed: {e}");
                    break;
                }
            }
        }
    });

    // frames: capture on a dedicated thread, encrypt (chunked), stream out.
    let (tx, mut rx) = mpsc::channel::<CapturedFrame>(2);
    std::thread::spawn(move || capture_loop(tx));
    while let Some(frame) = rx.recv().await {
        let header = FrameHeader {
            seq: frame.seq,
            width: frame.width,
            height: frame.height,
            stride: frame.stride,
            codec: frame.codec,
            capture_micros: frame.capture_micros,
            payload_len: frame.payload.len() as u32,
        };
        let mut plain = Vec::with_capacity(FrameHeader::ENCODED_LEN + frame.payload.len());
        plain.extend_from_slice(&header.encode());
        plain.extend_from_slice(&frame.payload);
        if let Err(e) = channel::write_secure(&mut send, &session, &plain).await {
            eprintln!("[host] frame stream ended: {e}");
            break;
        }
    }
    Ok(())
}
