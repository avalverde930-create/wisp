//! core::crypto — the ONLY home for crypto primitives.
//! Native E2EE: Noise_XX_25519_ChaChaPoly_BLAKE2s first contact (full mutual auth + SAS),
//! cache the peer static, Noise_IK for 0-RTT reconnect, CLEAN re-pair on IK decrypt failure.
//! XXfallback / Noise Pipes is intentionally NOT used — `snow` lacks the fallback modifier
//! and hand-rolling a handshake fallback is forbidden. See docs/adr/0003-noise-pattern.md.
//! FIPS note: if FIPS is required (decided Phase 0), swap to AES-GCM/SHA-2 BEFORE this freezes.
//! Fuzzed from Phase 0 (cargo-fuzz handshake target).
