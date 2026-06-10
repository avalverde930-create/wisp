//! client::net — Noise-secured QUIC networking. Connect, negotiate the handshake pattern
//! (IK 0-RTT reconnect when we have the host static cached, else XX first-contact), send
//! the access token, forward input, and receive the frame stream — all as chunked encrypted
//! records (`channel::{read_secure,write_secure}`) into shared state. Also the headless
//! `--bench` mode. The session AEAD is shared by the input task (encrypt) and the frame loop
//! (decrypt) via an `Arc<Mutex<_>>`.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use wisp_core::wire::{FrameHeader, InputEvent};
use wisp_core::{channel, codec, crypto, identity, known_hosts, transport, trust};

use crate::state::{LatestFrame, Shared};

/// Decode one Noise-decrypted frame plaintext (fixed header ++ LZ4 payload) to BGRA.
fn decode_frame_plaintext(pt: &[u8]) -> Result<(u32, u32, Vec<u8>)> {
    let header = FrameHeader::decode(pt).map_err(|e| anyhow::anyhow!("frame header: {e}"))?;
    let payload = &pt[FrameHeader::ENCODED_LEN..];
    let bgra = codec::decode_frame(header.codec, payload)?;
    Ok((header.width, header.height, bgra))
}

/// Load the persistent client device key (DPAPI-wrapped at rest, ADR-0009 Option A), or an
/// ephemeral key if there is no config dir.
fn load_device() -> Result<crypto::StaticKeypair> {
    match identity::role_key_path("client") {
        Some(p) => identity::load_or_create(p, identity::default_protector().as_ref()),
        None => crypto::generate_static_keypair(),
    }
}

/// Load the per-user client `KnownHosts` cache (falls back to a temp file if no config dir).
fn load_known_hosts() -> Result<known_hosts::KnownHosts> {
    let path = identity::role_key_path("client")
        .map(|p| p.with_file_name("known-hosts.txt"))
        .unwrap_or_else(|| std::env::temp_dir().join("wisp-known-hosts.txt"));
    known_hosts::KnownHosts::load(path)
}

/// Establish the secure channel. If we have a cached host static for this address, attempt
/// an IK 0-RTT reconnect; on any IK failure (e.g. the host rotated its key) drop the
/// connection and fall back to a fresh XX first-contact, re-learning the host static. A
/// changed host key on XX fallback is surfaced as a warning, not silently trusted (ADR-0003).
async fn establish(
    endpoint: &quinn::Endpoint,
    addr: SocketAddr,
    device_private: &[u8],
    known: &mut known_hosts::KnownHosts,
) -> Result<(
    quinn::Connection,
    quinn::SendStream,
    quinn::RecvStream,
    channel::Established,
)> {
    let target = addr.to_string();

    if let Some(host_static) = known.get(&target) {
        let conn = transport::connect(endpoint, addr).await?;
        let (mut send, mut recv) = conn.open_bi().await.context("open bi-stream (IK)")?;
        match channel::initiate_ik(&mut send, &mut recv, device_private, &host_static).await {
            Ok(est) => {
                println!(
                    "[client] 0-RTT reconnect (IK) to {target} [host {}]",
                    trust::fingerprint(&est.remote_static)
                );
                return Ok((conn, send, recv, est));
            }
            Err(e) => {
                eprintln!(
                    "[client] IK reconnect failed ({e}); falling back to a fresh XX handshake"
                );
                conn.close(0u32.into(), b"ik fallback");
            }
        }
    }

    let conn = transport::connect(endpoint, addr).await?;
    let (mut send, mut recv) = conn.open_bi().await.context("open bi-stream (XX)")?;
    let est = channel::initiate_xx(&mut send, &mut recv, device_private).await?;
    println!("[client] pairing SAS: {} (compare with the host)", est.sas);
    if let Some(old) = known.remember(&target, &est.remote_static)? {
        eprintln!(
            "[client] WARNING: host key for {target} CHANGED (was {}, now {}). \
             Re-paired on this connection; verify the SAS out-of-band before trusting it.",
            trust::fingerprint(&old),
            trust::fingerprint(&est.remote_static)
        );
    }
    Ok((conn, send, recv, est))
}

pub async fn net_main(
    addr: SocketAddr,
    shared: Arc<Shared>,
    mut input_rx: mpsc::UnboundedReceiver<InputEvent>,
    token: String,
) -> Result<()> {
    let device = load_device()?;
    let endpoint = transport::client_endpoint()?;
    let mut known = load_known_hosts()?;
    let (conn, mut send, mut recv, est) =
        establish(&endpoint, addr, &device.private, &mut known).await?;
    println!("[client] connected to {addr}");
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
        let device = load_device()?;
        let endpoint = transport::client_endpoint()?;
        let mut known = load_known_hosts()?;
        let (conn, mut send, mut recv, est) =
            establish(&endpoint, addr, &device.private, &mut known).await?;
        println!("[bench] connected to {addr}");
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
