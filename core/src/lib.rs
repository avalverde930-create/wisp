//! wisp-core — the shared Wisp core (MVP: one crate, many modules).
//! Splits into core/* crates in Phase 2-3. Imports nothing host/app/service-ward.

pub mod audit; // append-only hash-chained local log. Crypto-grade ownership.
pub mod codec; // frame (de)compression for the media path (Phase-0a: LZ4; 0b: HW H.264)
pub mod crypto; // Noise XX->IK; ONLY crypto home; fuzzed. Crypto-grade ownership.
pub mod identity; // device keys, secure element, revocation, recovery-code slot
pub mod media;
pub mod session; // pairing + consent + deny-by-default capabilities
pub mod transport; // quinn native data plane; packet parser is a fuzz target
pub mod wire; // hand-written wire structs (replaced by generated proto/ in Phase 2) // codec negotiation + capture/encode/input pipeline orchestration
