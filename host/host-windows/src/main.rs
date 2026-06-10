//! host-windows — Phase-0a host: GDI primary-monitor capture -> LZ4 -> quinn media
//! stream; client input stream -> Win32 `SendInput`. Interactive-session only (ADR-0010).
//!
//! SPIKE: the QUIC transport TLS is UNAUTHENTICATED (see core::transport). The real
//! authentication + confidentiality is the Phase-1 Noise XX/IK channel + SAS pairing
//! (ADR-0003). This binary proves the capture -> transport -> render -> input loop.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use srd_core::codec;
use srd_core::transport;
use srd_core::wire::{FrameCodec, FrameHeader, InputEvent, MouseButton};
use tokio::sync::mpsc;

use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE,
    MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEINPUT, MOUSE_EVENT_FLAGS,
    VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

/// One captured, already-compressed frame handed from the capture thread to the net task.
struct CapturedFrame {
    seq: u64,
    width: u32,
    height: u32,
    stride: u32,
    codec: FrameCodec,
    capture_micros: u64,
    payload: Vec<u8>,
}

fn primary_size() -> (i32, i32) {
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

fn capture_loop(tx: mpsc::Sender<CapturedFrame>) {
    let (w, h) = primary_size();
    let start = Instant::now();
    let mut seq = 0u64;
    let mut raw = Vec::new();
    let target = Duration::from_millis(33); // ~30 fps for the spike
    loop {
        let t0 = Instant::now();
        if capture_into(w, h, &mut raw).is_ok() {
            let (codec_tag, payload) = codec::encode_frame(&raw);
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

/// Inject one input event into the interactive session via SendInput.
/// Normalized coords map to 0..=65535 absolute over the primary monitor.
fn inject(ev: InputEvent) {
    unsafe {
        match ev {
            InputEvent::MouseMoveNorm { x, y } => {
                let dx = (x.clamp(0.0, 1.0) * 65535.0) as i32;
                let dy = (y.clamp(0.0, 1.0) * 65535.0) as i32;
                send_mouse(dx, dy, 0, MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE);
            }
            InputEvent::MouseButton { button, down } => {
                let flags = match (button, down) {
                    (MouseButton::Left, true) => MOUSEEVENTF_LEFTDOWN,
                    (MouseButton::Left, false) => MOUSEEVENTF_LEFTUP,
                    (MouseButton::Right, true) => MOUSEEVENTF_RIGHTDOWN,
                    (MouseButton::Right, false) => MOUSEEVENTF_RIGHTUP,
                    (MouseButton::Middle, true) => MOUSEEVENTF_MIDDLEDOWN,
                    (MouseButton::Middle, false) => MOUSEEVENTF_MIDDLEUP,
                };
                send_mouse(0, 0, 0, flags);
            }
            InputEvent::Wheel { delta } => {
                send_mouse(0, 0, delta * 120, MOUSEEVENTF_WHEEL);
            }
            InputEvent::Key {
                vk: _,
                scancode,
                down,
            } => {
                let mut flags = KEYEVENTF_SCANCODE;
                if !down {
                    flags |= KEYEVENTF_KEYUP;
                }
                send_key(scancode as u16, flags);
            }
        }
    }
}

unsafe fn send_mouse(dx: i32, dy: i32, mouse_data: i32, flags: MOUSE_EVENT_FLAGS) {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: mouse_data as u32,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
}

unsafe fn send_key(scan: u16, flags: KEYBD_EVENT_FLAGS) {
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
}

async fn handle_connection(conn: quinn::Connection, expected_token: Option<String>) -> Result<()> {
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
    let token = transport::read_hello(&mut input_recv)
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
            match transport::read_input(&mut input_recv).await {
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
        if let Err(e) = transport::write_frame(&mut send, &header, &frame.payload).await {
            eprintln!("[host] frame stream ended: {e}");
            break;
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let bind: SocketAddr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:9000".to_string())
        .parse()
        .context("parse bind addr (default 127.0.0.1:9000; LAN e.g. 0.0.0.0:9000)")?;

    // P1 guardrail: a non-loopback bind requires a shared token on BOTH ends, so a
    // LAN bind can never silently accept unauthenticated input injection.
    let token = std::env::var("SRD_SPIKE_TOKEN").ok();
    if !bind.ip().is_loopback() && token.is_none() {
        anyhow::bail!(
            "Refusing to bind {bind} (non-loopback) without a shared token.\n  \
             Set SRD_SPIKE_TOKEN on BOTH host and client first, e.g. (PowerShell):\n      \
             $env:SRD_SPIKE_TOKEN = 'choose-a-strong-secret'\n  \
             This is a spike guardrail against casual LAN access — NOT real security \
             (that is the Phase-1 Noise + SAS pairing)."
        );
    }

    let (w, h) = primary_size();
    let endpoint = transport::server_endpoint(bind)?;
    println!("[host] Secure Remote Desktop spike host");
    println!("[host] primary monitor: {w}x{h}");
    println!("[host] listening on {bind} (ALPN srd-spike/0) - waiting for a client...");
    match &token {
        Some(_) => println!("[host] auth: shared token REQUIRED (SRD_SPIKE_TOKEN)"),
        None => println!("[host] auth: NONE (loopback only; set SRD_SPIKE_TOKEN to allow LAN)"),
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
