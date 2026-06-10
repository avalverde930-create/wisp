//! wisp-core — the shared Wisp core (MVP: one crate, many modules).
//! Splits into core/* crates in Phase 2-3. Imports nothing host/app/service-ward.
//!
//! Module map (one responsibility each):
//! - `wire`      — hand-written wire structs + (de)serialization (frame header, input).
//! - `codec`     — frame (de)compression for the media path (Phase-0a: LZ4; 0b: HW H.264).
//! - `color`     — BGRA8 <-> NV12 colour conversion for the H.264 path (ADR-0011 4c).
//! - `transport` — quinn endpoint setup + spike TLS (the QUIC pipe).
//! - `framing`   — protocol I/O over quinn streams (frame / input / hello read+write).
//! - `crypto`    — Noise XX/IK handshake + session AEAD + SAS (ADR-0003).
//! - `channel`   — establish a Noise-secured session over a quinn bi-stream.
//! - `identity`  — persistent device keypair + at-rest protection (ADR-0009 Option A).
//! - `known_hosts` — client-side cache of host statics, keyed by address (IK reconnect).
//! - `trust`     — pinned peer public keys; reject an unknown static (ADR-0003).
//! - `audit` / `session` / `media` — Phase-1+ stubs (documented homes).

pub mod audit; // append-only hash-chained local log. Crypto-grade ownership.
pub mod channel; // establish a Noise-secured session over a quinn bi-stream
pub mod codec; // frame (de)compression for the media path (Phase-0a: LZ4; 0b: HW H.264)
pub mod color; // BGRA8 <-> NV12 colour conversion for the H.264 path (ADR-0011 4c)
pub mod crypto; // Noise XX->IK; ONLY crypto home; fuzzed. Crypto-grade ownership.
pub mod framing; // quinn-stream protocol I/O: frame / input / hello read+write
pub mod identity; // device keys, secure element, revocation, recovery-code slot
pub mod known_hosts; // client-side cache of host statics (addr -> static) for IK reconnect
pub mod media; // codec negotiation + capture/encode/input pipeline orchestration
pub mod session; // pairing + consent + deny-by-default capabilities
pub mod transport; // quinn endpoints + spike TLS; the native data plane
pub mod trust; // pinned peer public keys (ADR-0003 key pinning)
pub mod wire; // hand-written wire structs (replaced by generated proto/ in Phase 2)
