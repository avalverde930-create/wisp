//! host-windows — Phase-1 host: GDI primary-monitor capture -> LZ4 -> Noise-encrypted
//! quinn media stream; client input stream -> Win32 `SendInput`. Interactive-session
//! only (ADR-0010).
//!
//! Module map (one responsibility each):
//! - `capture`    — GDI screen grab + LZ4 encode on a dedicated thread.
//! - `inject`     — Win32 `SendInput` injection of inbound input.
//! - `connection` — one Noise-secured client session (handshake + encrypted streams).
//!
//! This file is just the entry point: arg parsing, the device keypair, the token
//! guardrail, and the accept loop.
//!
//! Traffic is now E2E-encrypted with Noise XX (`wisp_core::channel`) + an out-of-band
//! SAS (ADR-0003). The shared token remains the spike LAN access gate (sent encrypted).

mod capture;
mod connection;
mod inject;

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use wisp_core::{crypto, transport};

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
    // LAN bind can never silently accept unauthenticated control.
    let token = std::env::var("WISP_TOKEN").ok();
    if !bind.ip().is_loopback() && token.is_none() {
        anyhow::bail!(
            "Refusing to bind {bind} (non-loopback) without a shared token.\n  \
             Set WISP_TOKEN on BOTH host and client first, e.g. (PowerShell):\n      \
             $env:WISP_TOKEN = 'choose-a-strong-secret'\n  \
             The token is sent INSIDE the Noise channel; compare the printed SAS too."
        );
    }

    // Per-process device static key (ephemeral for the spike; persistence + OS-keystore
    // wrapping per ADR-0009 Option A is a later increment).
    let device_private = Arc::new(crypto::generate_static_keypair()?.private);

    let (w, h) = primary_size();
    let endpoint = transport::server_endpoint(bind)?;
    println!("[host] Wisp host");
    println!("[host] primary monitor: {w}x{h}");
    println!("[host] listening on {bind} (ALPN wisp/0) - waiting for a client...");
    println!("[host] transport: Noise XX E2E (a pairing SAS is printed per connection)");
    match &token {
        Some(_) => println!("[host] access: shared token REQUIRED (WISP_TOKEN)"),
        None => println!("[host] access: NONE (loopback only; set WISP_TOKEN to allow LAN)"),
    }
    println!("[host] NOTE: interactive session only; UAC / lock screen out of scope (ADR-0010).");

    while let Some(incoming) = endpoint.accept().await {
        let token = token.clone();
        let dp = device_private.clone();
        match incoming.await {
            Ok(conn) => {
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(conn, token, dp).await {
                        eprintln!("[host] connection error: {e}");
                    }
                });
            }
            Err(e) => eprintln!("[host] failed handshake: {e}"),
        }
    }
    Ok(())
}
