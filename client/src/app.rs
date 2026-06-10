//! client::app — the winit application: window creation, softbuffer present
//! (nearest-neighbor scaled to the window), and input capture (mouse/keyboard ->
//! normalized wire `InputEvent`s forwarded to the net thread). Phase-0b swaps the
//! softbuffer present for a wgpu GPU path behind this same surface.

use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use wisp_core::wire::{InputEvent, MouseButton as WireButton};

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::PhysicalKey;
use winit::platform::scancode::PhysicalKeyExtScancode;
use winit::window::{Window, WindowId};

use crate::state::Shared;

pub struct App {
    shared: Arc<Shared>,
    input_tx: mpsc::UnboundedSender<InputEvent>,
    window: Option<Arc<Window>>,
    // Kept alive for the surface's lifetime; not read directly.
    context: Option<softbuffer::Context<Arc<Window>>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    win_size: (u32, u32),
}

impl App {
    pub fn new(shared: Arc<Shared>, input_tx: mpsc::UnboundedSender<InputEvent>) -> Self {
        Self {
            shared,
            input_tx,
            window: None,
            context: None,
            surface: None,
            win_size: (1280, 720),
        }
    }

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
            w.set_title(&format!("Wisp — {:.0} fps · RTT {:.1} ms", s.fps, s.rtt_ms));
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = Window::default_attributes().with_title("Wisp — connecting…");
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
