//! core::codec — frame (de)compression for the media path.
//!
//! Phase-0a uses LZ4 (cheap, pure-Rust, keeps LAN bandwidth sane while we prove
//! the loop). Phase-0b replaces the encode call site with hardware H.264
//! (WGC -> NVENC/QSV/AMF, x264 floor) behind a `VideoEncoder` impl — the wire
//! `FrameCodec` tag already has room for it (`HwH264`).

use crate::wire::FrameCodec;
use anyhow::Result;

/// Compress one BGRA8 frame for the wire. Returns the codec tag + payload.
pub fn encode_frame(raw_bgra: &[u8]) -> (FrameCodec, Vec<u8>) {
    (
        FrameCodec::Lz4Bgra,
        lz4_flex::block::compress_prepend_size(raw_bgra),
    )
}

/// Decompress a wire payload back to BGRA8.
pub fn decode_frame(codec: FrameCodec, payload: &[u8]) -> Result<Vec<u8>> {
    match codec {
        FrameCodec::RawBgra => Ok(payload.to_vec()),
        FrameCodec::Lz4Bgra => lz4_flex::block::decompress_size_prepended(payload)
            .map_err(|e| anyhow::anyhow!("lz4 decompress: {e}")),
    }
}
