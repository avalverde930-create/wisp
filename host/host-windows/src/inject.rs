//! host-windows::inject — Win32 `SendInput` injection into the interactive session.
//!
//! Maps normalized wire `InputEvent`s onto absolute mouse moves (0..=65535 over the
//! primary monitor), button/wheel events, and scancode key events. Interactive session
//! only (ADR-0010): cannot drive UAC / the secure desktop / the lock screen.

use wisp_core::wire::{InputEvent, MouseButton};

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE,
    MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEINPUT, MOUSE_EVENT_FLAGS,
    VIRTUAL_KEY,
};

/// Inject one input event into the interactive session via `SendInput`.
/// Normalized coords map to 0..=65535 absolute over the primary monitor.
pub fn inject(ev: InputEvent) {
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
