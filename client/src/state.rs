//! client::state — shared state between the winit UI thread and the net thread.
//! The net thread writes the latest decoded frame + stats; the UI thread reads them on
//! redraw. Plain `Mutex` (short, uncontended locks — no async needed).

use std::sync::Mutex;

/// The most recent decoded frame (BGRA8), replaced in place by the net thread.
pub struct LatestFrame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// Latency numbers shown in the window title.
#[derive(Default, Clone, Copy)]
pub struct Stats {
    pub fps: f32,
    pub rtt_ms: f32,
}

pub struct Shared {
    pub frame: Mutex<Option<LatestFrame>>,
    pub stats: Mutex<Stats>,
}
