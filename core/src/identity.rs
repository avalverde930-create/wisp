//! core::identity — device keypair held in / wrapped by the secure element
//! (TPM 2.0 / Secure Enclave / StrongBox). NOTE (ADR-0009, Phase-0 spike): CNG/TPM expose
//! NIST curves, not X25519, so the X25519 Noise static may be OS-keystore-wrapped rather
//! than hardware-non-exportable; resolve before the key hierarchy freezes.
//! Software-key degradation only with a
//! NON-bypassable warning (never for unattended access or first pairing). Signed,
//! monotonically-versioned revocation (best-effort until the Phase-2 signaling layer
//! can reach offline devices). RESERVED SLOT: enrollment-time offline recovery code —
//! designed before the key hierarchy freezes (Phase 2) so it is never a bolt-on backdoor.
