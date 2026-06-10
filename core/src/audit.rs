//! core::audit — append-only, hash-chained, strictly-local audit log.
//! Records: session start/stop, peer key fingerprint, KEY-STORAGE CLASS per device
//! (hardware vs software), capabilities granted, file transfers, failed/declined attempts,
//! revocations. Never shipped to relay or cloud. Cross-device anchoring of log-head hashes
//! is DEFERRED to v1.0 (needs a 2nd device). Integrity model is easy to get subtly wrong —
//! crypto-grade CODEOWNERS alongside core::crypto.
