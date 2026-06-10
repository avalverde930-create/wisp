//! core::codec — frame (de)compression for the media path.
//!
//! Phase-0a was stateless LZ4 of each full BGRA frame. Phase-0b (ADR-0011 slice 4a) adds a
//! *stateful* interframe seam: `FrameEncoder` / `FrameDecoder` keep the previous frame and
//! emit either a KEYFRAME (full LZ4 frame, self-contained) or a DELTA (XOR vs the previous
//! frame, then LZ4) on a fixed GOP cadence. On a mostly-static desktop the XOR delta is
//! overwhelmingly zeros, so LZ4 crushes it — a large bandwidth (and thus fps) win.
//!
//! This same encoder-state seam is where the Phase-0b hardware H.264 encoder (ADR-0004,
//! slice 4c) slots in: H.264 is just another stateful impl with its own GOP/keyframe
//! structure, swapped in behind `FrameEncoder` / `FrameDecoder` — only the payload changes.

use crate::wire::FrameCodec;
use anyhow::Result;

/// Frames between forced keyframes. A keyframe lets a fresh or desynced decoder resync and
/// bounds error propagation; it is also the dimension-change reset point. QUIC streams are
/// reliable + ordered, so encoder and decoder stay in lockstep — the GOP is the resync floor.
pub const DEFAULT_GOP: u32 = 120; // ~4s at 30 fps

/// Stateless keyframe encode: LZ4 of the full BGRA frame (also the Phase-0a path).
pub fn encode_frame(raw_bgra: &[u8]) -> (FrameCodec, Vec<u8>) {
    (
        FrameCodec::Lz4Bgra,
        lz4_flex::block::compress_prepend_size(raw_bgra),
    )
}

/// Stateless decode for the self-contained codecs (keyframe / raw). An interframe `Lz4Delta`
/// payload needs decoder state and must go through [`FrameDecoder`].
pub fn decode_frame(codec: FrameCodec, payload: &[u8]) -> Result<Vec<u8>> {
    match codec {
        FrameCodec::RawBgra => Ok(payload.to_vec()),
        FrameCodec::Lz4Bgra => lz4_flex::block::decompress_size_prepended(payload)
            .map_err(|e| anyhow::anyhow!("lz4 decompress: {e}")),
        FrameCodec::Lz4Delta => {
            anyhow::bail!("Lz4Delta is an interframe codec; decode via FrameDecoder")
        }
    }
}

/// Stateful interframe encoder. Emits a keyframe at GOP boundaries and on a size change,
/// otherwise an XOR delta against the previous frame (LZ4-compressed).
pub struct FrameEncoder {
    prev: Vec<u8>,
    dims: (u32, u32),
    gop: u32,
    since_key: u32,
}

impl FrameEncoder {
    /// `gop` = max frames between keyframes (clamped to >= 1).
    pub fn new(gop: u32) -> Self {
        Self {
            prev: Vec::new(),
            dims: (0, 0),
            gop: gop.max(1),
            since_key: 0,
        }
    }

    /// Encode one BGRA frame; returns the wire codec tag + payload.
    pub fn encode(&mut self, raw_bgra: &[u8], width: u32, height: u32) -> (FrameCodec, Vec<u8>) {
        let dims = (width, height);
        let force_key =
            self.prev.len() != raw_bgra.len() || self.dims != dims || self.since_key >= self.gop;

        if force_key {
            let (tag, payload) = encode_frame(raw_bgra);
            self.remember(raw_bgra, dims);
            self.since_key = 1;
            return (tag, payload);
        }

        // XOR delta vs prev, then LZ4 (mostly-zeros on a static screen => tiny).
        let mut delta = vec![0u8; raw_bgra.len()];
        for ((d, &c), &p) in delta.iter_mut().zip(raw_bgra).zip(self.prev.iter()) {
            *d = c ^ p;
        }
        let payload = lz4_flex::block::compress_prepend_size(&delta);
        self.remember(raw_bgra, dims);
        self.since_key += 1;
        (FrameCodec::Lz4Delta, payload)
    }

    fn remember(&mut self, frame: &[u8], dims: (u32, u32)) {
        self.prev.clear();
        self.prev.extend_from_slice(frame);
        self.dims = dims;
    }
}

/// Stateful interframe decoder. Reconstructs each frame, keeping the previous one to apply
/// deltas. A delta before any keyframe is an error (the caller should wait for a keyframe).
pub struct FrameDecoder {
    prev: Vec<u8>,
}

impl FrameDecoder {
    pub fn new() -> Self {
        Self { prev: Vec::new() }
    }

    /// Decode one wire payload to a full BGRA frame, advancing decoder state.
    pub fn decode(&mut self, codec: FrameCodec, payload: &[u8]) -> Result<Vec<u8>> {
        match codec {
            FrameCodec::RawBgra | FrameCodec::Lz4Bgra => {
                let frame = decode_frame(codec, payload)?;
                self.prev.clone_from(&frame);
                Ok(frame)
            }
            FrameCodec::Lz4Delta => {
                anyhow::ensure!(!self.prev.is_empty(), "delta frame before any keyframe");
                let delta = lz4_flex::block::decompress_size_prepended(payload)
                    .map_err(|e| anyhow::anyhow!("lz4 delta decompress: {e}"))?;
                anyhow::ensure!(
                    delta.len() == self.prev.len(),
                    "delta size {} != previous frame {}",
                    delta.len(),
                    self.prev.len()
                );
                let mut frame = vec![0u8; delta.len()];
                for ((f, &d), &p) in frame.iter_mut().zip(&delta).zip(self.prev.iter()) {
                    *f = d ^ p;
                }
                self.prev.clone_from(&frame);
                Ok(frame)
            }
        }
    }
}

impl Default for FrameDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A keyframe then a delta then a delta all reconstruct exactly through the decoder.
    #[test]
    fn interframe_roundtrip_reconstructs() {
        let (w, h) = (4u32, 2u32);
        let n = (w * h * 4) as usize;
        let f0: Vec<u8> = (0..n).map(|i| (i % 251) as u8).collect();
        let mut f1 = f0.clone();
        f1[5] ^= 0x42; // small change
        let mut f2 = f1.clone();
        f2[20] = 0x99; // another small change

        let mut enc = FrameEncoder::new(120);
        let mut dec = FrameDecoder::new();

        let (c0, p0) = enc.encode(&f0, w, h);
        let (c1, p1) = enc.encode(&f1, w, h);
        let (c2, p2) = enc.encode(&f2, w, h);

        assert_eq!(c0, FrameCodec::Lz4Bgra); // first is always a keyframe
        assert_eq!(c1, FrameCodec::Lz4Delta);
        assert_eq!(c2, FrameCodec::Lz4Delta);

        assert_eq!(dec.decode(c0, &p0).unwrap(), f0);
        assert_eq!(dec.decode(c1, &p1).unwrap(), f1);
        assert_eq!(dec.decode(c2, &p2).unwrap(), f2);
    }

    /// GOP boundary forces a keyframe; a dimension change forces one too.
    #[test]
    fn keyframe_cadence_and_resize() {
        let (w, h) = (2u32, 2u32);
        let n = (w * h * 4) as usize;
        let frame: Vec<u8> = vec![7u8; n];

        let mut enc = FrameEncoder::new(3); // key every 3 frames
        let tags: Vec<FrameCodec> = (0..7).map(|_| enc.encode(&frame, w, h).0).collect();
        assert_eq!(
            tags,
            vec![
                FrameCodec::Lz4Bgra,  // 0: first => key
                FrameCodec::Lz4Delta, // 1
                FrameCodec::Lz4Delta, // 2
                FrameCodec::Lz4Bgra,  // 3: GOP boundary => key
                FrameCodec::Lz4Delta, // 4
                FrameCodec::Lz4Delta, // 5
                FrameCodec::Lz4Bgra,  // 6: GOP boundary => key
            ]
        );

        // A different size mid-stream forces a keyframe (prev no longer applies).
        let big: Vec<u8> = vec![1u8; (4 * 4 * 4) as usize];
        assert_eq!(enc.encode(&big, 4, 4).0, FrameCodec::Lz4Bgra);
    }

    /// A delta arriving before any keyframe is rejected (decoder has no reference frame).
    #[test]
    fn delta_before_keyframe_errors() {
        let mut dec = FrameDecoder::new();
        let bogus = lz4_flex::block::compress_prepend_size(&[0u8; 16]);
        assert!(dec.decode(FrameCodec::Lz4Delta, &bogus).is_err());
    }

    /// On a fully static screen the delta is all zeros => far smaller than a realistic
    /// (high-entropy) keyframe. Uses an LCG so the keyframe is genuinely incompressible,
    /// the way real desktop pixels are — a flat/repeating synthetic frame would let LZ4
    /// crush BOTH to one giant match and make the comparison meaningless.
    #[test]
    fn static_delta_is_tiny() {
        let (w, h) = (640u32, 480u32);
        let n = (w * h * 4) as usize;
        let mut x: u32 = 0x1234_5678;
        let frame: Vec<u8> = (0..n)
            .map(|_| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (x >> 24) as u8
            })
            .collect();
        let mut enc = FrameEncoder::new(120);
        let (_, key) = enc.encode(&frame, w, h);
        let (tag, delta) = enc.encode(&frame, w, h); // identical next frame
        assert_eq!(tag, FrameCodec::Lz4Delta);
        assert!(
            delta.len() * 20 < key.len(),
            "static delta {} should be a small fraction of keyframe {}",
            delta.len(),
            key.len()
        );
    }
}
