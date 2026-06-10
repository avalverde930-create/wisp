//! wisp-media-win — Windows-only media codec glue, shared by the host (encoder) and client
//! (decoder). Currently the Media Foundation H.264 encode/decode path (ADR-0011 4c). The
//! cross-platform colour conversion lives in `wisp_core::color`; this crate is the Windows
//! Media Foundation boundary, kept out of the portable `core`.

pub mod h264;
