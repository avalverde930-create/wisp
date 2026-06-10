//! host-windows::connection — one authenticated client session.
//!
//! Validates the spike pre-shared token on the control stream first, withholding the
//! screen AND input injection until it passes. After auth: a task injects inbound input
//! (`inject`), while the main task streams captured frames (`capture`) out over a uni
//! stream. Real auth is the Phase-1 Noise + SAS pairing (ADR-0003).

use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use wisp_core::framing;
use wisp_core::wire::FrameHeader;

use crate::capture::{capture_loop, CapturedFrame};
use crate::inject::inject;

pub async fn handle_connection(
    conn: quinn::Connection,
    expected_token: Option<String>,
) -> Result<()> {
    let peer = conn.remote_address();

    // SPIKE AUTH: the client must open the control stream and present the shared
    // token first. We withhold the screen AND input injection until it validates.
    // The token crosses an unauthenticated (cert-skipped) channel, so this guards
    // against casual/opportunistic LAN access, NOT an active MITM. Real auth is the
    // Phase-1 Noise XX/IK + SAS pairing (ADR-0003).
    let mut input_recv = match tokio::time::timeout(Duration::from_secs(5), conn.accept_uni()).await
    {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => anyhow::bail!("accept control stream from {peer}: {e}"),
        Err(_) => anyhow::bail!("client {peer} sent no control stream within 5s"),
    };
    let token = framing::read_hello(&mut input_recv)
        .await
        .context("read client hello")?;
    if let Some(expected) = &expected_token {
        if &token != expected {
            eprintln!("[host] REJECTED {peer}: bad token");
            conn.close(1u32.into(), b"bad token");
            return Ok(());
        }
    }
    println!("[host] client authenticated: {peer}");

    // input: inject everything that arrives on the (now-authenticated) control stream.
    tokio::spawn(async move {
        loop {
            match framing::read_input(&mut input_recv).await {
                Ok(ev) => inject(ev),
                Err(e) => {
                    eprintln!("[host] input loop ended: {e}");
                    break;
                }
            }
        }
    });

    // frames: capture on a dedicated thread, stream over one uni channel.
    let (tx, mut rx) = mpsc::channel::<CapturedFrame>(2);
    std::thread::spawn(move || capture_loop(tx));

    let mut send = conn.open_uni().await.context("open frame stream")?;
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
        if let Err(e) = framing::write_frame(&mut send, &header, &frame.payload).await {
            eprintln!("[host] frame stream ended: {e}");
            break;
        }
    }
    Ok(())
}
