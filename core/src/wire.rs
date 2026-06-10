//! Hand-written wire protocol for the MVP. The single source of truth shared by
//! host and client via this crate. Replaced by generated `proto/wisp/v1` types in
//! Phase 2 (buf+prost), then ts-proto in Phase 3. Three channels, ALL inside the
//! Noise envelope (including bulk — no file-transfer exemption).
//
// Channels: Media (unreliable), Control (reliable/ordered), Bulk (reliable/consent-gated).
// Capability negotiation, never assumption: HELLO advertises codecs/hw/max_res.
//
// PHASE-0/1 SPIKE NOTE: this is the cleartext framing carried INSIDE the QUIC
// transport. In Phase 1 proper every frame additionally rides inside the Noise
// envelope (ADR-0003); the Phase-0 spike runs WITHOUT Noise to first prove the
// capture -> transport -> render -> input loop end to end on the LAN.

use thiserror::Error;

/// "WSP1" — magic prefix on every media frame header.
pub const PROTOCOL_MAGIC: u32 = 0x5753_5031;
/// Bump on any wire-incompatible change (the spike is v1).
pub const PROTOCOL_VERSION: u8 = 1;
/// Hard cap on a peer-declared frame payload. `decode` (and thus `read_frame`) rejects
/// anything larger BEFORE allocating, so a malicious host / MITM on the cert-skipped
/// spike transport cannot force a giant allocation. 64 MiB covers a 4K raw BGRA frame
/// (~32 MiB) with headroom; LZ4 payloads are smaller still.
pub const MAX_FRAME_PAYLOAD: u32 = 64 * 1024 * 1024;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WireError {
    #[error("buffer too short: need {need}, have {have}")]
    TooShort { need: usize, have: usize },
    #[error("bad magic: {0:#010x}")]
    BadMagic(u32),
    #[error("unsupported protocol version: {0}")]
    BadVersion(u8),
    #[error("frame payload {len} exceeds max {max}")]
    FrameTooLarge { len: u32, max: u32 },
    #[error("unknown frame codec: {0}")]
    UnknownCodec(u8),
    #[error("unknown input tag: {0}")]
    UnknownTag(u8),
    #[error("unknown mouse button: {0}")]
    UnknownButton(u8),
}

// ---------------------------------------------------------------------------
// Media frames
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameCodec {
    /// Uncompressed BGRA8 (loopback / debugging only — huge).
    RawBgra = 0,
    /// LZ4-compressed BGRA8 keyframe (Phase-0a default; also the GOP keyframe in 0b).
    Lz4Bgra = 1,
    /// LZ4-compressed XOR delta vs the previous frame (Phase-0b interframe, ADR-0011 4a).
    /// Self-describing only with decoder state — decode via `codec::FrameDecoder`.
    Lz4Delta = 2,
    // Phase-0b/1: HwH264 = 3 (WGC -> NVENC/QSV/AMF, x264 floor).
}

impl FrameCodec {
    pub fn from_u8(v: u8) -> Result<Self, WireError> {
        match v {
            0 => Ok(FrameCodec::RawBgra),
            1 => Ok(FrameCodec::Lz4Bgra),
            2 => Ok(FrameCodec::Lz4Delta),
            other => Err(WireError::UnknownCodec(other)),
        }
    }
}

/// Fixed-size header that precedes every media-frame payload on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    pub seq: u64,
    pub width: u32,
    pub height: u32,
    /// Source row stride in bytes of the *decoded* BGRA image.
    pub stride: u32,
    pub codec: FrameCodec,
    /// Host capture timestamp, microseconds since the host process start
    /// (a monotonic clock). Used for cadence/latency telemetry; cross-machine
    /// glass-to-glass uses RTT (quinn) since wall clocks are not synchronized.
    pub capture_micros: u64,
    /// Length in bytes of the payload that follows this header.
    pub payload_len: u32,
}

impl FrameHeader {
    pub const ENCODED_LEN: usize = 4 + 1 + 1 + 2 + 8 + 4 + 4 + 4 + 8 + 4; // = 40

    pub fn encode(&self) -> [u8; Self::ENCODED_LEN] {
        let mut b = [0u8; Self::ENCODED_LEN];
        let mut o = 0usize;
        write_u32(&mut b, &mut o, PROTOCOL_MAGIC);
        b[o] = PROTOCOL_VERSION;
        o += 1;
        b[o] = self.codec as u8;
        o += 1;
        // 2 bytes reserved/padding
        o += 2;
        write_u64(&mut b, &mut o, self.seq);
        write_u32(&mut b, &mut o, self.width);
        write_u32(&mut b, &mut o, self.height);
        write_u32(&mut b, &mut o, self.stride);
        write_u64(&mut b, &mut o, self.capture_micros);
        write_u32(&mut b, &mut o, self.payload_len);
        debug_assert_eq!(o, Self::ENCODED_LEN);
        b
    }

    pub fn decode(buf: &[u8]) -> Result<Self, WireError> {
        if buf.len() < Self::ENCODED_LEN {
            return Err(WireError::TooShort {
                need: Self::ENCODED_LEN,
                have: buf.len(),
            });
        }
        let mut o = 0usize;
        let magic = read_u32(buf, &mut o);
        if magic != PROTOCOL_MAGIC {
            return Err(WireError::BadMagic(magic));
        }
        let version = buf[o];
        o += 1;
        if version != PROTOCOL_VERSION {
            return Err(WireError::BadVersion(version));
        }
        let codec = FrameCodec::from_u8(buf[o])?;
        o += 1;
        o += 2; // reserved
        let seq = read_u64(buf, &mut o);
        let width = read_u32(buf, &mut o);
        let height = read_u32(buf, &mut o);
        let stride = read_u32(buf, &mut o);
        let capture_micros = read_u64(buf, &mut o);
        let payload_len = read_u32(buf, &mut o);
        if payload_len > MAX_FRAME_PAYLOAD {
            return Err(WireError::FrameTooLarge {
                len: payload_len,
                max: MAX_FRAME_PAYLOAD,
            });
        }
        Ok(FrameHeader {
            seq,
            width,
            height,
            stride,
            codec,
            capture_micros,
            payload_len,
        })
    }
}

// ---------------------------------------------------------------------------
// Input events (client -> host control channel)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MouseButton {
    Left = 0,
    Right = 1,
    Middle = 2,
}

impl MouseButton {
    pub fn from_u8(v: u8) -> Result<Self, WireError> {
        match v {
            0 => Ok(MouseButton::Left),
            1 => Ok(MouseButton::Right),
            2 => Ok(MouseButton::Middle),
            other => Err(WireError::UnknownButton(other)),
        }
    }
}

/// Input is sent in **normalized** coordinates (0.0..=1.0 across the host's
/// primary monitor) so the client window size never has to match the host
/// resolution — the host maps normalized -> pixels at injection time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputEvent {
    MouseMoveNorm { x: f32, y: f32 },
    MouseButton { button: MouseButton, down: bool },
    Wheel { delta: i32 },
    Key { vk: u32, scancode: u32, down: bool },
}

impl InputEvent {
    const TAG_MOVE: u8 = 1;
    const TAG_BUTTON: u8 = 2;
    const TAG_WHEEL: u8 = 3;
    const TAG_KEY: u8 = 4;

    /// Encode to a self-delimiting byte buffer (tag + fixed fields per tag).
    pub fn encode(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(16);
        match *self {
            InputEvent::MouseMoveNorm { x, y } => {
                v.push(Self::TAG_MOVE);
                v.extend_from_slice(&x.to_bits().to_be_bytes());
                v.extend_from_slice(&y.to_bits().to_be_bytes());
            }
            InputEvent::MouseButton { button, down } => {
                v.push(Self::TAG_BUTTON);
                v.push(button as u8);
                v.push(down as u8);
            }
            InputEvent::Wheel { delta } => {
                v.push(Self::TAG_WHEEL);
                v.extend_from_slice(&delta.to_be_bytes());
            }
            InputEvent::Key { vk, scancode, down } => {
                v.push(Self::TAG_KEY);
                v.extend_from_slice(&vk.to_be_bytes());
                v.extend_from_slice(&scancode.to_be_bytes());
                v.push(down as u8);
            }
        }
        v
    }

    /// Decode one event from the front of `buf`, returning it and the number of
    /// bytes consumed (so a stream reader can advance).
    pub fn decode(buf: &[u8]) -> Result<(InputEvent, usize), WireError> {
        let need = |n: usize| {
            if buf.len() < n {
                Err(WireError::TooShort {
                    need: n,
                    have: buf.len(),
                })
            } else {
                Ok(())
            }
        };
        need(1)?;
        let tag = buf[0];
        let mut o = 1usize;
        match tag {
            Self::TAG_MOVE => {
                need(1 + 8)?;
                let x = f32::from_bits(read_u32(buf, &mut o));
                let y = f32::from_bits(read_u32(buf, &mut o));
                Ok((InputEvent::MouseMoveNorm { x, y }, o))
            }
            Self::TAG_BUTTON => {
                need(1 + 2)?;
                let button = MouseButton::from_u8(buf[o])?;
                o += 1;
                let down = buf[o] != 0;
                o += 1;
                Ok((InputEvent::MouseButton { button, down }, o))
            }
            Self::TAG_WHEEL => {
                need(1 + 4)?;
                let delta = read_u32(buf, &mut o) as i32;
                Ok((InputEvent::Wheel { delta }, o))
            }
            Self::TAG_KEY => {
                need(1 + 9)?;
                let vk = read_u32(buf, &mut o);
                let scancode = read_u32(buf, &mut o);
                let down = buf[o] != 0;
                o += 1;
                Ok((InputEvent::Key { vk, scancode, down }, o))
            }
            other => Err(WireError::UnknownTag(other)),
        }
    }
}

// ---------------------------------------------------------------------------
// little big-endian cursor helpers (no external dep)
// ---------------------------------------------------------------------------

#[inline]
fn write_u32(b: &mut [u8], o: &mut usize, v: u32) {
    b[*o..*o + 4].copy_from_slice(&v.to_be_bytes());
    *o += 4;
}
#[inline]
fn write_u64(b: &mut [u8], o: &mut usize, v: u64) {
    b[*o..*o + 8].copy_from_slice(&v.to_be_bytes());
    *o += 8;
}
#[inline]
fn read_u32(b: &[u8], o: &mut usize) -> u32 {
    let v = u32::from_be_bytes([b[*o], b[*o + 1], b[*o + 2], b[*o + 3]]);
    *o += 4;
    v
}
#[inline]
fn read_u64(b: &[u8], o: &mut usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[*o..*o + 8]);
    *o += 8;
    u64::from_be_bytes(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_header_rejects_oversized_payload() {
        let h = FrameHeader {
            seq: 0,
            width: 1,
            height: 1,
            stride: 4,
            codec: FrameCodec::RawBgra,
            capture_micros: 0,
            payload_len: MAX_FRAME_PAYLOAD + 1,
        };
        let bytes = h.encode();
        assert!(matches!(
            FrameHeader::decode(&bytes),
            Err(WireError::FrameTooLarge { .. })
        ));
    }

    #[test]
    fn frame_header_roundtrip() {
        let h = FrameHeader {
            seq: 42,
            width: 1920,
            height: 1080,
            stride: 1920 * 4,
            codec: FrameCodec::Lz4Bgra,
            capture_micros: 123_456_789,
            payload_len: 4096,
        };
        let bytes = h.encode();
        assert_eq!(bytes.len(), FrameHeader::ENCODED_LEN);
        assert_eq!(FrameHeader::decode(&bytes).unwrap(), h);
    }

    #[test]
    fn frame_header_rejects_bad_magic() {
        let mut bytes = FrameHeader {
            seq: 1,
            width: 1,
            height: 1,
            stride: 4,
            codec: FrameCodec::RawBgra,
            capture_micros: 0,
            payload_len: 0,
        }
        .encode();
        bytes[0] ^= 0xFF;
        assert!(matches!(
            FrameHeader::decode(&bytes),
            Err(WireError::BadMagic(_))
        ));
    }

    #[test]
    fn input_event_roundtrip() {
        let events = [
            InputEvent::MouseMoveNorm { x: 0.5, y: 0.25 },
            InputEvent::MouseButton {
                button: MouseButton::Right,
                down: true,
            },
            InputEvent::Wheel { delta: -3 },
            InputEvent::Key {
                vk: 0x41,
                scancode: 30,
                down: false,
            },
        ];
        for e in events {
            let buf = e.encode();
            let (decoded, used) = InputEvent::decode(&buf).unwrap();
            assert_eq!(decoded, e);
            assert_eq!(used, buf.len());
        }
    }

    #[test]
    fn input_event_short_buffer() {
        assert!(matches!(
            InputEvent::decode(&[]),
            Err(WireError::TooShort { .. })
        ));
        assert!(matches!(
            InputEvent::decode(&[InputEvent::TAG_KEY]),
            Err(WireError::TooShort { .. })
        ));
    }
}
