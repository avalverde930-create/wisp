# ADR-0006 — Pairing: device-bound secure-element keys + out-of-band SAS; no account until v1.0

- **Status:** Accepted.
- **Date:** 2026-06-09.
- **Related:** `docs/SECURITY.md` §4.3, `docs/security/crypto-spec.md` §2–3, ADR-0003 (Noise), ADR-0009 (device-key storage).

## Context

Reaching your own machine should not require an account, a password, or a third party. The MVP must
defeat pairing-time MITM without any server (it is LAN-only and zero-server at cold start).

## Decision

- **Device identity = a static keypair held in / wrapped by the secure element.** The X25519-vs-
  TPM/CNG storage question is a Phase-0 spike — see **ADR-0009**.
- **Pairing ceremony:** run `Noise_XX`; derive a **Short Authentication String** bound to the full
  handshake transcript; the owner compares it **out-of-band** (same screen / in-person QR / voice).
  **No blind trust-on-first-use.** Pin the peer key thereafter; a changed key triggers a loud
  warning and forces re-pair.
- **No account in the MVP.** Device pairing is the only auth; there is no password to phish or
  brute-force. Account login (**OPAQUE**, RFC 9807) ships **with accounts/teams in v1.0**, not
  before.
- **Hardened degradation:** software-protected keys are allowed only with a non-bypassable warning,
  **never** for unattended access or first pairing; the audit log records key-storage class.

## Consequences

- The trust root is the secure-element key + the human SAS comparison — no CA, no account server in
  the MVP path.
- Revocation is signed + monotonically-versioned from day one, but **best-effort until Phase 2**
  (no rendezvous to reach an offline device in a pure-P2P MVP) — stated in product and docs.
- An enrollment-time offline **recovery code** is designed before the key hierarchy freezes
  (implemented Phase 2) so device loss is not a permanent host lockout and is never a bolt-on
  backdoor.
