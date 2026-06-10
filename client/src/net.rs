//! client::net — QUIC client networking: connect + authenticate (spike token), receive
//! and decode the frame stream into shared state, and forward input events. Also the
//! headless `--bench` mode (no window; prints fps + RTT). `transport` builds/dials the
//! pipe; `framing` does the per-stream protocol I/O.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use wisp_core::wire::InputEvent;
use wisp_core::{codec, framing, transport};

use crate::state::{LatestFrame, Shared};

pub async fn net_main(
    addr: SocketAddr,
    shared: Arc<Shared>,
    mut input_rx: mpsc::UnboundedReceiver<InputEvent>,
    token: String,
) -> Result<()> {
    let endpoint = transport::client_endpoint()?;
    let conn = transport::connect(&endpoint, addr).await?;
    println!("[client] connected to {addr}");

    // input: open a uni stream, authenticate (spike token), then drain UI events.
    let conn_in = conn.clone();
    tokio::spawn(async move {
        match conn_in.open_uni().await {
            Ok(mut send) => {
                if let Err(e) = framing::write_hello(&mut send, &token).await {
                    eprintln!("[client] hello failed: {e}");
                    return;
                }
                while let Some(ev) = input_rx.recv().await {
                    if framing::write_input(&mut send, &ev).await.is_err() {
                        break;
                    }
                }
            }
            Err(e) => eprintln!("[client] open input stream: {e}"),
        }
    });

    // frames: accept the host's uni stream, decode, store latest, update stats.
    let mut recv = conn.accept_uni().await.context("accept frame stream")?;
    let mut count = 0u32;
    let mut last = Instant::now();
    loop {
        let (header, payload) = framing::read_frame(&mut recv).await?;
        let bgra = codec::decode_frame(header.codec, &payload)?;
        *shared.frame.lock().unwrap() = Some(LatestFrame {
            width: header.width,
            height: header.height,
            bgra,
        });
        count += 1;
        let dt = last.elapsed().as_secs_f32();
        if dt >= 0.5 {
            let mut s = shared.stats.lock().unwrap();
            s.fps = count as f32 / dt;
            s.rtt_ms = conn.rtt().as_secs_f32() * 1000.0;
            count = 0;
            last = Instant::now();
        }
    }
}

/// Headless smoke test: connect, receive frames for a few seconds, verify decode, and
/// print the visible latency numbers (fps + QUIC RTT). No window. `client <addr> --bench`.
pub fn run_bench(addr: SocketAddr, token: String) -> Result<()> {
    let rt = tokio::runtime::Runtime::new().context("tokio runtime")?;
    rt.block_on(async move {
        let endpoint = transport::client_endpoint()?;
        let conn = transport::connect(&endpoint, addr).await?;
        println!("[bench] connected to {addr}");
        // authenticate (spike token) on the control stream, then receive frames.
        let mut _ctrl = conn.open_uni().await.context("open control stream")?;
        framing::write_hello(&mut _ctrl, &token).await?;
        let mut recv = conn.accept_uni().await.context("accept frame stream")?;
        let start = Instant::now();
        let (mut count, mut bytes) = (0u64, 0u64);
        let mut dims = (0u32, 0u32);
        while start.elapsed() < Duration::from_secs(6) {
            let (header, payload) = framing::read_frame(&mut recv).await?;
            let bgra = codec::decode_frame(header.codec, &payload)?;
            anyhow::ensure!(
                bgra.len() == (header.width as usize) * (header.height as usize) * 4,
                "decoded size {} != {}x{}x4",
                bgra.len(),
                header.width,
                header.height
            );
            count += 1;
            bytes += payload.len() as u64;
            dims = (header.width, header.height);
        }
        let secs = start.elapsed().as_secs_f64();
        println!(
            "[bench] {count} frames in {secs:.1}s = {:.1} fps | {}x{} | avg {} KiB/frame on the wire | RTT {:.2} ms | decode OK",
            count as f64 / secs,
            dims.0,
            dims.1,
            bytes / count.max(1) / 1024,
            conn.rtt().as_secs_f64() * 1000.0
        );
        anyhow::Ok(())
    })
}
