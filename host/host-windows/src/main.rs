//! host-windows — Phase-0a host: GDI primary-monitor capture -> LZ4 -> quinn media
//! stream; client input stream -> Win32 `SendInput`. Interactive-session only (ADR-0010).
//!
//! Module map (one responsibility each):
//! - `capture`    — GDI screen grab + LZ4 encode on a dedicated thread.
//! - `inject`     — Win32 `SendInput` injection of inbound input.
//! - `connection` — one authenticated client session (auth + stream wiring).
//!
//! This file is just the entry point: arg parsing, the token guardrail, and the
//! accept loop.
//!
//! SPIKE: the QUIC transport TLS is UNAUTHENTICATED (see `wisp_core::transport`). Real
//! auth is the Phase-1 Noise XX/IK + SAS pairing (ADR-0003).

mod capture;
mod connection;
mod inject;

use std::net::SocketAddr;

use anyhow::{Context, Result};
use wisp_core::transport;

use crate::capture::primary_size;
use crate::connection::handle_connection;

#[tokio::main]
async fn main() -> Result<()> {
    let bind: SocketAddr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:9000".to_string())
        .parse()
        .context("parse bind addr (default 127.0.0.1:9000; LAN e.g. 0.0.0.0:9000)")?;

    // P1 guardrail: a non-loopback bind requires a shared token on BOTH ends, so a
    // LAN bind can never silently accept unauthenticated input injection.
    let token = std::env::var("WISP_TOKEN").ok();
    if !bind.ip().is_loopback() && token.is_none() {
        anyhow::bail!(
            "Refusing to bind {bind} (non-loopback) without a shared token.\n  \
             Set WISP_TOKEN on BOTH host and client first, e.g. (PowerShell):\n      \
             $env:WISP_TOKEN = 'choose-a-strong-secret'\n  \
             This is a spike guardrail against casual LAN access — NOT real security \
             (that is the Phase-1 Noise + SAS pairing)."
        );
    }

    let (w, h) = primary_size();
    let endpoint = transport::server_endpoint(bind)?;
    println!("[host] Wisp spike host");
    println!("[host] primary monitor: {w}x{h}");
    println!("[host] listening on {bind} (ALPN wisp/0) - waiting for a client...");
    match &token {
        Some(_) => println!("[host] auth: shared token REQUIRED (WISP_TOKEN)"),
        None => println!("[host] auth: NONE (loopback only; set WISP_TOKEN to allow LAN)"),
    }
    println!("[host] NOTE: interactive session only; UAC / lock screen out of scope (ADR-0010).");

    while let Some(incoming) = endpoint.accept().await {
        let token = token.clone();
        match incoming.await {
            Ok(conn) => {
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(conn, token).await {
                        eprintln!("[host] connection error: {e}");
                    }
                });
            }
            Err(e) => eprintln!("[host] failed handshake: {e}"),
        }
    }
    Ok(())
}
