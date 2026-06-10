//! host-windows::connection — one Noise-secured client session.
//!
//! The client opens a bi-stream; we run the Noise XX handshake (responder) to get an
//! E2E session + the out-of-band SAS, then the client's first secure message is the
//! access token (the spike LAN guardrail). After that the bi-stream carries everything
//! encrypted (chunked Noise records via `channel::{read_secure,write_secure}`): inbound
//! input (decrypt -> inject) on the recv half, outbound captured frames (encrypt -> send)
//! on the send half. The session AEAD is shared via an `Arc<Mutex<_>>`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use wisp_core::channel;
use wisp_core::wire::{FrameHeader, InputEvent};

use crate::capture::{capture_loop, CapturedFrame};
use crate::inject::inject;

pub async fn handle_connection(
    conn: quinn::Connection,
    expected_token: Option<String>,
    device_private: Arc<Vec<u8>>,
) -> Result<()> {
    let peer = conn.remote_address();

    // The client opens the bi-stream; we are the Noise XX responder. The handshake gives
    // mutual auth + the SAS, and the session encrypts everything that follows.
    let (mut send, mut recv) =
        match tokio::time::timeout(Duration::from_secs(5), conn.accept_bi()).await {
            Ok(Ok(x)) => x,
            Ok(Err(e)) => anyhow::bail!("accept stream from {peer}: {e}"),
            Err(_) => anyhow::bail!("client {peer} opened no stream within 5s"),
        };
    let est = channel::handshake_xx_responder(&mut send, &mut recv, &device_private)
        .await
        .with_context(|| format!("noise handshake with {peer}"))?;
    println!(
        "[host] pairing SAS for {peer}: {} (compare with the client)",
        est.sas
    );
    let session = Arc::new(Mutex::new(est.session));

    // First secure message = the access token (the spike LAN guardrail).
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
        // plaintext = fixed header ++ (already-LZ4) payload, then Noise-encrypt (chunked).
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
