//! client — Phase-0a desktop client (winit + softbuffer).
//!
//! Module map (one responsibility each):
//! - `state` — shared data between the UI thread and the net thread.
//! - `app`   — the winit window: render (softbuffer) + input capture.
//! - `net`   — QUIC connect/receive/forward, plus the headless `--bench` mode.
//!
//! This file is the entry point: arg parsing, the net thread, and the winit event loop.
//!
//! SPIKE: transport is unauthenticated (see `wisp_core::transport`); real auth is Phase-1 Noise.

mod app;
mod net;
mod state;

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use wisp_core::wire::InputEvent;

use winit::event_loop::{ControlFlow, EventLoop};

use crate::app::App;
use crate::net::{net_main, run_bench};
use crate::state::{Shared, Stats};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let bench = args.iter().any(|a| a == "--bench");
    let addr: SocketAddr = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| "127.0.0.1:9000".to_string())
        .parse()
        .context("parse host addr (e.g. 192.168.1.50:9000)")?;

    let token = std::env::var("WISP_TOKEN").unwrap_or_default();

    if bench {
        return run_bench(addr, token);
    }

    let shared = Arc::new(Shared {
        frame: Mutex::new(None),
        stats: Mutex::new(Stats::default()),
    });
    let (input_tx, input_rx) = mpsc::unbounded_channel::<InputEvent>();

    // networking runs on its own tokio runtime thread; winit owns the main thread.
    let net_shared = shared.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async move {
            if let Err(e) = net_main(addr, net_shared, input_rx, token).await {
                eprintln!("[client] net error: {e}");
            }
        });
    });

    let event_loop = EventLoop::new().context("create event loop")?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::new(shared, input_tx);
    event_loop.run_app(&mut app).context("run event loop")?;
    Ok(())
}
