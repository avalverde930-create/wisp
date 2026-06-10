//! host-windows — Phase-1 host: GDI primary-monitor capture -> LZ4 -> Noise-encrypted
//! quinn media stream; client input -> Win32 `SendInput`. Interactive-session only (ADR-0010).
//!
//! Module map (one responsibility each):
//! - `capture`     — GDI screen grab + interframe encode on a dedicated thread.
//! - `capture_wgc` — Windows.Graphics.Capture path (ADR-0011 4b; 4b.0 = capability probe).
//! - `inject`      — Win32 `SendInput` injection of inbound input.
//! - `connection`  — one Noise-secured, pinned client session.
//!
//! This file is the entry point: arg parsing, the persistent device key, the pinned
//! trust store, the token guardrail, and the accept loop.
//!
//! Security: Noise XX E2E (`wisp_core::channel`) + out-of-band SAS (ADR-0003) + key
//! PINNING (ADR-0003): a non-loopback client whose device key is not pinned is rejected
//! unless the host runs in pair mode (WISP_PAIR=1).

mod capture;
mod capture_wgc;
mod connection;
mod h264;
mod inject;

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use wisp_core::trust::{self, TrustStore};
use wisp_core::{crypto, identity, transport};

use crate::capture::primary_size;
use crate::connection::handle_connection;

#[tokio::main]
async fn main() -> Result<()> {
    // WGC capability probe (ADR-0011 4b.0): `host-windows.exe --probe-wgc`. Confirms
    // Windows.Graphics.Capture initializes on this machine, prints the WGC monitor size, exits.
    if std::env::args().any(|a| a == "--probe-wgc") {
        match capture_wgc::probe() {
            Ok((w, h)) => {
                println!(
                    "[host] WGC OK: Windows.Graphics.Capture initialized; primary monitor {w}x{h}"
                )
            }
            Err(e) => println!("[host] WGC unavailable: {e:#}"),
        }
        return Ok(());
    }

    // Hardware H.264 encoder probe (ADR-0011 4c.0): `host-windows.exe --probe-h264`. Lists the
    // H.264 encoder MFTs (hardware NVENC/QSV/AMF + software floor) available on this machine.
    if std::env::args().any(|a| a == "--probe-h264") {
        if let Err(e) = h264::probe() {
            println!("[host] H.264 probe failed: {e:#}");
        }
        return Ok(());
    }

    let bind: SocketAddr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:9000".to_string())
        .parse()
        .context("parse bind addr (default 127.0.0.1:9000; LAN e.g. 0.0.0.0:9000)")?;

    // P1 guardrail: a non-loopback bind requires a shared token on BOTH ends.
    let token = std::env::var("WISP_TOKEN").ok();
    if !bind.ip().is_loopback() && token.is_none() {
        anyhow::bail!(
            "Refusing to bind {bind} (non-loopback) without a shared token.\n  \
             Set WISP_TOKEN on BOTH host and client first, e.g. (PowerShell):\n      \
             $env:WISP_TOKEN = 'choose-a-strong-secret'\n  \
             The token is sent INSIDE the Noise channel; compare the printed SAS too."
        );
    }

    // Persistent device static key (stable identity across runs), wrapped at rest by the
    // default protector — Windows DPAPI per ADR-0009 Option A (`identity::default_protector`).
    let device = match identity::role_key_path("host") {
        Some(p) => identity::load_or_create(p, identity::default_protector().as_ref())?,
        None => {
            eprintln!("[host] no config dir found; using an ephemeral device key");
            crypto::generate_static_keypair()?
        }
    };
    let device_private = Arc::new(device.private);

    // Pinned trust store of known client device keys (ADR-0003 key pinning).
    let trust_path = identity::role_key_path("host")
        .map(|p| p.with_file_name("trusted-clients.txt"))
        .unwrap_or_else(|| std::env::temp_dir().join("wisp-trusted-clients.txt"));
    let trust = Arc::new(Mutex::new(TrustStore::load(&trust_path)?));
    let pair_mode = std::env::var("WISP_PAIR").is_ok();

    let (w, h) = primary_size();
    let endpoint = transport::server_endpoint(bind)?;
    println!("[host] Wisp host");
    println!(
        "[host] device fingerprint: {}",
        trust::fingerprint(&device.public)
    );
    println!("[host] primary monitor: {w}x{h}");
    println!("[host] listening on {bind} (ALPN wisp/0) - waiting for a client...");
    println!("[host] transport: Noise XX E2E + key pinning (SAS printed per connection)");
    println!(
        "[host] pairing: {}",
        if pair_mode {
            "ON (new LAN devices will be pinned)".to_string()
        } else {
            format!(
                "off ({} pinned; set WISP_PAIR=1 to pair a new LAN device)",
                trust.lock().unwrap().len()
            )
        }
    );
    match &token {
        Some(_) => println!("[host] access: shared token REQUIRED (WISP_TOKEN)"),
        None => println!("[host] access: token NONE (loopback only)"),
    }
    println!("[host] NOTE: interactive session only; UAC / lock screen out of scope (ADR-0010).");

    while let Some(incoming) = endpoint.accept().await {
        let token = token.clone();
        let dp = device_private.clone();
        let tr = trust.clone();
        match incoming.await {
            Ok(conn) => {
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(conn, token, dp, tr, pair_mode).await {
                        eprintln!("[host] connection error: {e}");
                    }
                });
            }
            Err(e) => eprintln!("[host] failed handshake: {e}"),
        }
    }
    Ok(())
}
