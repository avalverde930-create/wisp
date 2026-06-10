//! core::session — pairing + consent state machine; deny-by-default capability model
//! (VIEW/CONTROL/CLIPBOARD/FILE_TRANSFER/AUDIO/MULTI_MONITOR as separate grants; default
//! view-only). Grant table carries expires_at/consent_proof for the future assist-others case.
//! Capability TOKENS and FIDO2 attestation are DEFERRED to srd/v2 (don't freeze a half-spec'd
//! security schema). Input replay protection: drop queued inputs on resume after background.
