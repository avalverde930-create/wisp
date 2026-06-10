//! client::net — Noise-secured QUIC networking. Connect, run the Noise XX handshake
//! (initiator) over a bi-stream, send the access token, forward input, and receive the
//! frame stream — all as chunked encrypted records (`channel::{read_secure,write_secure}`)
//! into shared state. Also the headless `--bench` mode. The session AEAD is shared by the
//! input task (encrypt) and the frame loop (decrypt) via an `Arc<Mutex<_>>`.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use wisp_core::wire::{FrameHeader, InputEvent};
use wisp_core::{channel, codec, crypto, identity, transport};

use crate::state::{LatestFrame, Shared};

/// Decode one Noise-decrypted frame plaintext (fixed header ++ LZ4 payload) to BGRA.
fn decode_frame_plaintext(pt: &[u8]) -> Result<(u32, u32, Vec<u8>)> {
    let header = FrameHeader::decode(pt).map_err(|e| anyhow::anyhow!("frame header: {e}"))?;
    let payload = &pt[FrameHeader::ENCODED_LEN..];
    let bgra = codec::decode_frame(header.codec, payload)?;
    Ok((header.width, header.height, bgra))
}

pub async fn net_main(
    addr: SocketAddr,
    shared: Arc<Shared>,
    mut input_rx: mpsc::UnboundedReceiver<InputEvent>,
    token: String,
) -> Result<()> {
    let device = match identity::role_key_path("client") {
        Some(p) => identity::load_or_create(p, &identity::Unprotected)?,
        None => crypto::generate_static_keypair()?,
    };
    let endpoint = transport::client_endpoint()?;
    let conn = transport::connect(&endpoint, addr).await?;
    println!("[client] connected to {addr}");

    let (mut send, mut recv) = conn.open_bi().await.context("open bi-stream")?;
    let est = channel::handshake_xx_initiator(&mut send, &mut recv, &device.private)
        .await
        .context("noise handshake")?;
    println!("[client] pairing SAS: {} (compare with the host)", est.sas);
    let session = Arc::new(Mutex::new(est.session));

    // First secure message = the access token (the spike LAN guardrail).
    channel::write_secure(&mut send, &session, token.as_bytes())
        .await
        .context("send token")?;

    // input: encrypt UI events and send them.
    let in_session = session.clone();
    tokio::spawn(async move {
        while let Some(ev) = input_rx.recv().await {
            if channel::write_secure(&mut send, &in_session, &ev.encode())
                .await
                .is_err()
            {
                break;
            }
        }
    });

    // frames: receive, decrypt, parse, store latest, update stats.
    let mut count = 0u32;
    let mut last = Instant::now();
    loop {
        let pt = channel::read_secure(&mut recv, &session).await?;
        let (width, height, bgra) = decode_frame_plaintext(&pt)?;
        *shared.frame.lock().unwrap() = Some(LatestFrame {
            width,
            height,
            bgra,
        });
        count += 1;
        let dt = last.elapsed().as_secs_f32();
        if dt >= 0.5 {
            let mut st = shared.stats.lock().unwrap();
            st.fps = count as f32 / dt;
            st.rtt_ms = conn.rtt().as_secs_f32() * 1000.0;
            count = 0;
            last = Instant::now();
        }
    }
}

/// Headless smoke test: handshake, send token, receive + decrypt frames for a few
/// seconds, verify decode, and print the latency numbers. No window. `client <addr> --bench`.
pub fn run_bench(addr: SocketAddr, token: String) -> Result<()> {
    let rt = tokio::runtime::Runtime::new().context("tokio runtime")?;
    rt.block_on(async move {
        let device = match identity::role_key_path("client") {
        Some(p) => identity::load_or_create(p, &identity::Unprotected)?,
        None => crypto::generate_static_keypair()?,
    };
        let endpoint = transport::client_endpoint()?;
        let conn = transport::connect(&endpoint, addr).await?;
        println!("[bench] connected to {addr}");
        let (mut send, mut recv) = conn.open_bi().await.context("open bi-stream")?;
        let est = channel::handshake_xx_initiator(&mut send, &mut recv, &device.private)
            .await
            .context("noise handshake")?;
        println!("[bench] pairing SAS: {}", est.sas);
        let session = Mutex::new(est.session);
        channel::write_secure(&mut send, &session, token.as_bytes())
            .await
            .context("send token")?;

        let start = Instant::now();
        let (mut count, mut bytes) = (0u64, 0u64);
        let mut dims = (0u32, 0u32);
        while start.elapsed() < Duration::from_secs(6) {
            let pt = channel::read_secure(&mut recv, &session).await?;
            let (width, height, bgra) = decode_frame_plaintext(&pt)?;
            anyhow::ensure!(
                bgra.len() == (width as usize) * (height as usize) * 4,
                "decoded size {} != {width}x{height}x4",
                bgra.len()
            );
            count += 1;
            bytes += pt.len() as u64;
            dims = (width, height);
        }
        let secs = start.elapsed().as_secs_f64();
        println!(
            "[bench] {count} frames in {secs:.1}s = {:.1} fps | {}x{} | avg {} KiB/frame (Noise plaintext) | RTT {:.2} ms | decode OK",
            count as f64 / secs,
            dims.0,
            dims.1,
            bytes / count.max(1) / 1024,
            conn.rtt().as_secs_f64() * 1000.0
        );
        anyhow::Ok(())
    })
}
