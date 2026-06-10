//! host-windows::capture — GDI primary-monitor screen capture.
//!
//! Grabs the primary monitor as top-down BGRA8 on a dedicated OS thread, encodes each frame
//! through a stateful `wisp_core::codec::FrameEncoder` (GOP keyframe + XOR-delta interframe,
//! ADR-0011 4a), and hands them to the net task over a bounded channel (back-pressure paces
//! capture). Phase-0b 4b/4c replace the GDI grab with WGC and the encoder with hardware H.264
//! behind the same `FrameEncoder` call site.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use wisp_core::codec;
use wisp_core::wire::FrameCodec;

use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

/// One captured, already-compressed frame handed from the capture thread to the net task.
pub struct CapturedFrame {
    pub seq: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub codec: FrameCodec,
    pub capture_micros: u64,
    pub payload: Vec<u8>,
}

/// Primary-monitor pixel size as (width, height).
pub fn primary_size() -> (i32, i32) {
    unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
}

/// Capture the primary monitor into `buf` as top-down BGRA8. Blocking GDI; runs on
/// a dedicated OS thread (no COM/STA requirement for GDI).
fn capture_into(width: i32, height: i32, buf: &mut Vec<u8>) -> Result<()> {
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            anyhow::bail!("GetDC(primary) failed");
        }
        let mem = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, width, height);
        let old = SelectObject(mem, bmp);

        let blit = BitBlt(mem, 0, 0, width, height, screen, 0, 0, SRCCOPY);

        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height, // negative => top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        buf.resize((width as usize) * (height as usize) * 4, 0);
        let scanlines = GetDIBits(
            mem,
            bmp,
            0,
            height as u32,
            Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
            &mut info,
            DIB_RGB_COLORS,
        );

        // cleanup regardless
        SelectObject(mem, old);
        let _ = DeleteObject(bmp);
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);

        blit.context("BitBlt")?;
        if scanlines == 0 {
            anyhow::bail!("GetDIBits returned 0 scanlines");
        }
    }
    Ok(())
}

/// Capture loop (~30 fps): grab -> interframe-encode -> push to `tx`. Exits when the receiver
/// (the net task) drops, which is how a disconnect tears the capture thread down.
pub fn capture_loop(tx: mpsc::Sender<CapturedFrame>) {
    let (w, h) = primary_size();
    let start = Instant::now();
    let mut seq = 0u64;
    let mut raw = Vec::new();
    let mut encoder = codec::FrameEncoder::new(codec::DEFAULT_GOP);
    let target = Duration::from_millis(33); // ~30 fps for the spike
    loop {
        let t0 = Instant::now();
        if capture_into(w, h, &mut raw).is_ok() {
            let (codec_tag, payload) = encoder.encode(&raw, w as u32, h as u32);
            let frame = CapturedFrame {
                seq,
                width: w as u32,
                height: h as u32,
                stride: (w * 4) as u32,
                codec: codec_tag,
                capture_micros: start.elapsed().as_micros() as u64,
                payload,
            };
            seq = seq.wrapping_add(1);
            // blocking_send applies back-pressure; errors when the receiver (net task) drops.
            if tx.blocking_send(frame).is_err() {
                break;
            }
        } else {
            std::thread::sleep(Duration::from_millis(100));
        }
        let dt = t0.elapsed();
        if dt < target {
            std::thread::sleep(target - dt);
        }
    }
}
