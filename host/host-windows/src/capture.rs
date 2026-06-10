//! host-windows::capture — GDI primary-monitor screen capture.
//!
//! Grabs the primary monitor as top-down BGRA8 on a dedicated OS thread, encodes each frame,
//! and hands them to the net task over a bounded channel (back-pressure paces capture). The
//! encoder is selectable (ADR-0011): `WISP_CODEC=h264` uses the Media Foundation H.264 encoder
//! (`wisp-media-win`, 4c), otherwise the default LZ4 GOP+XOR-delta interframe codec (4a). The
//! capture source is selectable too: `WISP_CAPTURE=wgc` for Windows.Graphics.Capture (4b).

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use wisp_core::wire::FrameCodec;
use wisp_core::{codec, color};
use wisp_media_win::h264::H264Encoder;

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

/// The active frame encoder: the default LZ4 interframe codec, or the Media Foundation H.264
/// encoder (`WISP_CODEC=h264`). Both consume BGRA and yield a wire `FrameCodec` + payload.
enum FrameEncoderKind {
    Interframe(codec::FrameEncoder),
    H264(H264Encoder),
}

impl FrameEncoderKind {
    fn encode(&mut self, bgra: &[u8], w: u32, h: u32) -> Result<(FrameCodec, Vec<u8>)> {
        match self {
            FrameEncoderKind::Interframe(e) => Ok(e.encode(bgra, w, h)),
            FrameEncoderKind::H264(e) => {
                let nv12 = color::bgra_to_nv12(bgra, w, h);
                Ok((FrameCodec::HwH264, e.encode(&nv12)?))
            }
        }
    }
}

/// Build the encoder for `w`x`h` from `WISP_CODEC` (falls back to the LZ4 interframe codec if
/// H.264 is requested but unavailable).
fn make_encoder(w: u32, h: u32) -> FrameEncoderKind {
    let want_h264 = std::env::var("WISP_CODEC")
        .map(|v| v.eq_ignore_ascii_case("h264"))
        .unwrap_or(false);
    if want_h264 {
        match H264Encoder::new_software(w, h, 30, 8_000_000) {
            Ok(e) => {
                println!("[host] codec: H.264 (software MFT, low-latency)");
                return FrameEncoderKind::H264(e);
            }
            Err(e) => {
                eprintln!("[host] H.264 requested but unavailable ({e:#}); using LZ4 interframe")
            }
        }
    }
    println!("[host] codec: LZ4 interframe");
    FrameEncoderKind::Interframe(codec::FrameEncoder::new(codec::DEFAULT_GOP))
}

/// Encode one BGRA frame and send it; returns false when the receiver (net task) has dropped
/// (the disconnect signal that tears the capture thread down). An empty payload (the H.264
/// encoder is still buffering) is skipped, not sent. `blocking_send` back-pressures.
fn encode_and_send(
    tx: &mpsc::Sender<CapturedFrame>,
    encoder: &mut FrameEncoderKind,
    seq: &mut u64,
    start: Instant,
    w: u32,
    h: u32,
    bgra: &[u8],
) -> bool {
    let (codec, payload) = match encoder.encode(bgra, w, h) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("[host] encode error: {e}");
            return true; // skip this frame, keep capturing
        }
    };
    if payload.is_empty() {
        return true; // encoder buffering — nothing to send yet
    }
    let frame = CapturedFrame {
        seq: *seq,
        width: w,
        height: h,
        stride: w * 4,
        codec,
        capture_micros: start.elapsed().as_micros() as u64,
        payload,
    };
    *seq = seq.wrapping_add(1);
    tx.blocking_send(frame).is_ok()
}

/// Capture loop: pick the capture source, then grab -> interframe-encode -> push to `tx`.
/// `WISP_CAPTURE=wgc` selects Windows.Graphics.Capture (ADR-0011 4b); otherwise (and on any
/// WGC init failure) the default GDI grab is used. Exits when the net task drops the receiver.
pub fn capture_loop(tx: mpsc::Sender<CapturedFrame>) {
    let want_wgc = std::env::var("WISP_CAPTURE")
        .map(|v| v.eq_ignore_ascii_case("wgc"))
        .unwrap_or(false);
    if want_wgc {
        match crate::capture_wgc::WgcCapturer::new() {
            Ok(cap) => {
                println!(
                    "[host] capture: Windows.Graphics.Capture ({}x{}, native pixels)",
                    cap.width(),
                    cap.height()
                );
                wgc_capture_loop(cap, tx);
                return;
            }
            Err(e) => eprintln!("[host] WGC requested but unavailable ({e:#}); using GDI"),
        }
    }
    println!("[host] capture: GDI");
    gdi_capture_loop(tx);
}

/// GDI capture loop (~30 fps): BitBlt + GetDIBits the primary monitor (DPI-scaled logical res).
fn gdi_capture_loop(tx: mpsc::Sender<CapturedFrame>) {
    let (w, h) = primary_size();
    let start = Instant::now();
    let mut seq = 0u64;
    let mut raw = Vec::new();
    let mut encoder = make_encoder(w as u32, h as u32);
    let target = Duration::from_millis(33); // ~30 fps for the spike
    loop {
        let t0 = Instant::now();
        if capture_into(w, h, &mut raw).is_ok() {
            if !encode_and_send(&tx, &mut encoder, &mut seq, start, w as u32, h as u32, &raw) {
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

/// 4c.3b self-test: capture `frames` real GDI frames and H.264-encode them (the live capture
/// path), returning (frames captured, total H.264 bytes, frames that produced output, whether
/// an Annex-B start code is present). Verifies the real-capture -> H.264 path in isolation.
pub fn selftest_capture_h264(frames: usize) -> Result<(usize, usize, usize, bool)> {
    let (w, h) = primary_size();
    let mut enc = H264Encoder::new_software(w as u32, h as u32, 30, 8_000_000)?;
    let mut raw = Vec::new();
    let mut stream = Vec::new();
    let (mut captured, mut with_output) = (0usize, 0usize);
    for _ in 0..frames {
        if capture_into(w, h, &mut raw).is_ok() {
            captured += 1;
            let nv12 = color::bgra_to_nv12(&raw, w as u32, h as u32);
            let nal = enc.encode(&nv12)?;
            if !nal.is_empty() {
                with_output += 1;
            }
            stream.extend_from_slice(&nal);
        }
        std::thread::sleep(Duration::from_millis(33));
    }
    stream.extend_from_slice(&enc.drain()?);
    let start_code = stream.windows(4).any(|x| x == [0, 0, 0, 1]);
    Ok((captured, stream.len(), with_output, start_code))
}

/// WGC capture loop: poll the frame pool; WGC delivers a frame when the desktop changes, so a
/// static screen yields none (efficient). Captures at the monitor's native resolution.
fn wgc_capture_loop(mut cap: crate::capture_wgc::WgcCapturer, tx: mpsc::Sender<CapturedFrame>) {
    let start = Instant::now();
    let mut seq = 0u64;
    let mut encoder = make_encoder(cap.width(), cap.height());
    let target = Duration::from_millis(16); // poll ~60 fps; None when nothing changed
    loop {
        let t0 = Instant::now();
        match cap.try_next() {
            Ok(Some((w, h, bgra))) => {
                if !encode_and_send(&tx, &mut encoder, &mut seq, start, w, h, &bgra) {
                    break;
                }
            }
            Ok(None) => {}
            Err(e) => eprintln!("[host] WGC frame error: {e}"),
        }
        let dt = t0.elapsed();
        if dt < target {
            std::thread::sleep(target - dt);
        }
    }
}
