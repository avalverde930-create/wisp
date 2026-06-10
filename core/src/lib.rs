//! wisp-core — the shared Wisp core (MVP: one crate, many modules).
//! Splits into core/* crates in Phase 2-3. Imports nothing host/app/service-ward.
//!
//! Module map (one responsibility each):
//! - `wire`      — hand-written wire structs + (de)serialization (frame header, input).
//! - `codec`     — frame (de)compression for the media path (Phase-0a: LZ4; 0b: HW H.264).
//! - `transport` — quinn endpoint setup + spike TLS (the QUIC pipe).
//! - `framing`   — protocol I/O over quinn streams (frame / input / hello read+write).
//! - `crypto` / `audit` / `identity` / `session` / `media` — Phase-1+ stubs (documented homes).

pub mod audit; // append-only hash-chained local log. Crypto-grade ownership.
pub mod codec; // frame (de)compression for the media path (Phase-0a: LZ4; 0b: HW H.264)
pub mod crypto; // Noise XX->IK; ONLY crypto home; fuzzed. Crypto-grade ownership.
pub mod framing; // quinn-stream protocol I/O: frame / input / hello read+write
pub mod identity; // device keys, secure element, revocation, recovery-code slot
pub mod media; // codec negotiation + capture/encode/input pipeline orchestration
pub mod session; // pairing + consent + deny-by-default capabilities
pub mod transport; // quinn endpoints + spike TLS; the native data plane
pub mod wire; // hand-written wire structs (replaced by generated proto/ in Phase 2)
