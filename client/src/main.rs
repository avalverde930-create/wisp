//! client — Phase-0a desktop client (winit + softbuffer).
//!
//! Connects to the host over quinn (core::transport), decodes the LZ4 BGRA media
//! stream (core::codec), presents it with softbuffer (nearest-neighbor scaled to the
//! window), and sends winit input back as normalized `InputEvent`s. The window title
//! shows the visible latency numbers: received FPS + QUIC path RTT.
//!
//! SPIKE: transport is unauthenticated (see core::transport); real auth is Phase-1 Noise.

use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use srd_core::codec;
use srd_core::transport;
use srd_core::wire::{InputEvent, MouseButton as WireButton};
use tokio::sync::mpsc;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::platform::scancode::PhysicalKeyExtScancode;
use winit::window::{Window, WindowId};

struct LatestFrame {
    width: u32,
    height: u32,
    bgra: Vec<u8>,
}

#[derive(Default, Clone, Copy)]
struct Stats {
    fps: f32,
    rtt_ms: f32,
}

struct Shared {
    frame: Mutex<Option<LatestFrame>>,
    stats: Mutex<Stats>,
}

struct App {
    shared: Arc<Shared>,
    input_tx: mpsc::UnboundedSender<InputEvent>,
    window: Option<Arc<Window>>,
    context: Option<softbuffer::Context<Arc<Window>>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    win_size: (u32, u32),
}

impl App {
    fn redraw(&mut self) {
        let (ww, wh) = self.win_size;
        let (Some(surface), (Some(nzw), Some(nzh))) = (
            self.surface.as_mut(),
            (NonZeroU32::new(ww), NonZeroU32::new(wh)),
        ) else {
            return;
        };
        if surface.resize(nzw, nzh).is_err() {
            return;
        }
        let mut buffer = match surface.buffer_mut() {
            Ok(b) => b,
            Err(_) => return,
        };
        for p in buffer.iter_mut() {
            *p = 0x0010_1014; // dark background
        }
        if let Some(f) = self.shared.frame.lock().unwrap().as_ref() {
            let (fw, fh) = (f.width as usize, f.height as usize);
            if fw > 0 && fh > 0 {
                let (ww, wh) = (ww as usize, wh as usize);
                for yy in 0..wh {
                    let sy = yy * fh / wh;
                    let row = sy * fw;
                    let dst_row = yy * ww;
                    for xx in 0..ww {
                        let sx = xx * fw / ww;
                        let si = (row + sx) * 4;
                        if si + 2 < f.bgra.len() {
                            let b = f.bgra[si] as u32;
                            let g = f.bgra[si + 1] as u32;
                            let r = f.bgra[si + 2] as u32;
                            buffer[dst_row + xx] = (r << 16) | (g << 8) | b;
                        }
                    }
                }
            }
        }
        let _ = buffer.present();

        if let Some(w) = &self.window {
            let s = *self.shared.stats.lock().unwrap();
            w.set_title(&format!(
                "SRD spike — {:.0} fps · RTT {:.1} ms",
                s.fps, s.rtt_ms
            ));
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = Window::default_attributes().with_title("SRD spike — connecting…");
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        let context = softbuffer::Context::new(window.clone()).expect("softbuffer context");
        let surface =
            softbuffer::Surface::new(&context, window.clone()).expect("softbuffer surface");
        let size = window.inner_size();
        self.win_size = (size.width.max(1), size.height.max(1));
        self.window = Some(window);
        self.context = Some(context);
        self.surface = Some(surface);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.win_size = (size.width.max(1), size.height.max(1));
            }
            WindowEvent::CursorMoved { position, .. } => {
                let (w, h) = self.win_size;
                let nx = (position.x as f32 / w as f32).clamp(0.0, 1.0);
                let ny = (position.y as f32 / h as f32).clamp(0.0, 1.0);
                let _ = self
                    .input_tx
                    .send(InputEvent::MouseMoveNorm { x: nx, y: ny });
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(b) = map_button(button) {
                    let _ = self.input_tx.send(InputEvent::MouseButton {
                        button: b,
                        down: state == ElementState::Pressed,
                    });
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let d = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y.round() as i32,
                    MouseScrollDelta::PixelDelta(p) => (p.y / 40.0).round() as i32,
                };
                if d != 0 {
                    let _ = self.input_tx.send(InputEvent::Wheel { delta: d });
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(_) = event.physical_key {
                    if let Some(sc) = event.physical_key.to_scancode() {
                        let _ = self.input_tx.send(InputEvent::Key {
                            vk: 0,
                            scancode: sc,
                            down: event.state == ElementState::Pressed,
                        });
                    }
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // Cap redraw cadence so we don't spin a core at 100%.
        std::thread::sleep(Duration::from_millis(6));
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

fn map_button(b: MouseButton) -> Option<WireButton> {
    match b {
        MouseButton::Left => Some(WireButton::Left),
        MouseButton::Right => Some(WireButton::Right),
        MouseButton::Middle => Some(WireButton::Middle),
        _ => None,
    }
}

async fn net_main(
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
                if let Err(e) = transport::write_hello(&mut send, &token).await {
                    eprintln!("[client] hello failed: {e}");
                    return;
                }
                while let Some(ev) = input_rx.recv().await {
                    if transport::write_input(&mut send, &ev).await.is_err() {
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
        let (header, payload) = transport::read_frame(&mut recv).await?;
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

/// Headless smoke test: connect, receive frames for a few seconds, verify decode,
/// and print the visible latency numbers (fps + QUIC RTT). No window. `client <addr> --bench`.
fn run_bench(addr: SocketAddr, token: String) -> Result<()> {
    let rt = tokio::runtime::Runtime::new().context("tokio runtime")?;
    rt.block_on(async move {
        let endpoint = transport::client_endpoint()?;
        let conn = transport::connect(&endpoint, addr).await?;
        println!("[bench] connected to {addr}");
        // authenticate (spike token) on the control stream, then receive frames.
        let mut _ctrl = conn.open_uni().await.context("open control stream")?;
        transport::write_hello(&mut _ctrl, &token).await?;
        let mut recv = conn.accept_uni().await.context("accept frame stream")?;
        let start = Instant::now();
        let (mut count, mut bytes) = (0u64, 0u64);
        let mut dims = (0u32, 0u32);
        while start.elapsed() < Duration::from_secs(6) {
            let (header, payload) = transport::read_frame(&mut recv).await?;
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

    let token = std::env::var("SRD_SPIKE_TOKEN").unwrap_or_default();

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
    let mut app = App {
        shared,
        input_tx,
        window: None,
        context: None,
        surface: None,
        win_size: (1280, 720),
    };
    event_loop.run_app(&mut app).context("run event loop")?;
    Ok(())
}
