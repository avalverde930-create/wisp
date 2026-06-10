# Cryptography Specification

> Companion to `docs/SECURITY.md`, ADR-0003 (Noise pattern), and ADR-0009 (device-key storage).
> This is the v0 spec; suites are not frozen until the Phase-0 FIPS + device-key gate.
> **Do not hand-roll any of this** — compose vetted libraries; engineering effort goes to the
> trust model, defaults, key lifecycle, and consent UX.

## 1. Native E2EE (MVP / v1) — Noise

- **First contact:** `Noise_XX_25519_ChaChaPoly_BLAKE2s` — full mutual authentication, with a
  Short Authentication String (SAS) verifiable out-of-band.
- **Reconnect:** cache the peer static key after XX; use `Noise_IK` for 0-RTT reconnect.
- **Key rotation:** on an `IK` decrypt failure (host static rotated), perform a **clean SAS
  re-pair** — **never** an automatic XXfallback.
- **Why not Noise Pipes / XXfallback:** the `snow` crate does **not** implement the fallback
  modifier, and hand-rolling a handshake fallback is forbidden (it would violate do-not-hand-roll
  in the worst possible place). The SAS re-pair on key change is something the trust model already
  requires. Recorded in ADR-0003.
- **Library:** `snow` (Noise) over `ring` / `aws-lc-rs` primitives; `libsodium` (`dryoc`) where a
  vetted primitive is cleaner. AEAD/ECDH/hash/RNG are never hand-implemented.

## 2. Pairing ceremony (defeats pairing-time MITM)

Run Noise XX; derive a **SAS** (numeric/emoji) bound to the **full handshake transcript**; the
owner compares it **out-of-band** (same screen / in-person QR / voice). **No blind trust-on-first-
use.** Pin the peer key thereafter; a changed key triggers a loud warning and forces re-pair.

## 3. Device identity & key storage (see ADR-0009)

- Target: a static keypair **held in / wrapped by the secure element** — Windows TPM 2.0 via CNG,
  Apple Secure Enclave, Android StrongBox/TEE.
- **Composition risk — DECIDED (ADR-0009, Option A):** CNG/TPM and the FIPS Platform Crypto Provider
  expose **NIST curves (P-256/384), not Curve25519**, while Noise uses **X25519**. The MVP uses
  **Option A: non-FIPS X25519 wrapped at rest by the OS keystore** (DPAPI / CNG software KSP) — not
  hardware-non-exportable. Options (b) FIPS/NIST-curve and (c) TPM-sealed wrapping stay recorded for a
  future enterprise / FIPS posture. So "non-exportable in the TPM" is explicitly **not** an MVP
  guarantee; the storage class is recorded per device.
- **Degradation:** software-protected keys are allowed **only** with a non-bypassable warning,
  **never** for unattended access or first pairing; the audit log records **key-storage class
  (hardware vs software) per device**.

## 4. Key hierarchy

`secure-element device key → per-session ephemeral (Noise XX/IK, forward-secret) → (browser only)
per-frame SFrame ratchet`. Rekey within long sessions. **Per-session forward secrecy** throughout.

- **Revocation (day one):** any device revocable from any other; propagated as a **signed,
  monotonically-versioned** trust-list update; endpoints reject versions older than last-seen.
  *Best-effort until Phase 2* (no rendezvous to reach an offline device in a P2P MVP).
- **Recovery:** an enrollment-time **offline recovery code** is designed before the key hierarchy
  freezes (implemented Phase 2) so it is never a late bolt-on backdoor. Slot reserved in
  `core/identity` + the wire schema now.

## 5. Browser E2EE (Phase 3 / v1.0)

- WebRTC **DTLS 1.3** transport **plus a mandatory second E2EE layer**: **SFrame** via
  Encoded-Transform / Insertable-Streams, keyed from a **Noise/ECDH agreement over the data
  channel** (not from DTLS) — so a TURN relay / SFU forwards only opaque frames.
- **Refuse rather than downgrade:** if a browser lacks Insertable-Streams/Encoded-Transform,
  refuse the session rather than silently weaken it.
- **WebKit/Safari support is AT-RISK, not excluded:** resolved by a **Phase-3 compatibility spike**
  (Open Q #13). Chromium-family is the confirmed target; Safari/iOS-web ships iff the spike
  confirms a working Encoded Transform + SFrame path, otherwise iOS uses the native UniFFI client.
- SFrame stays **native-client-free in the MVP**: native 1:1 P2P is already blinded by the Noise
  tunnel, so per-frame SFrame is only needed for browser/relay-forwarding (SFU-style) and future
  multi-party "assist" cases.

## 6. FIPS variant — DECIDED 2026-06-09: not for the MVP

**The MVP is non-FIPS:** ChaCha20-Poly1305 / BLAKE2s + X25519 (ADR-0009 Option A). FIPS is a
**later enterprise constraint**, not built now. If a future customer compliance bar requires it,
substitute **AES-GCM / SHA-2** for ChaCha20-Poly1305 / BLAKE2s in the Noise suite and move the
device key onto a NIST curve (ADR-0009 Option B) — the cipher suite and key hierarchy were kept
swappable behind ADR-0003 / ADR-0009 precisely so this stays a configuration change, not a rewrite.

## 7. At-rest & accounts

- **Local at-rest passphrase:** Argon2id (OWASP-2025 params).
- **Account login (v1.0 only):** OPAQUE (augmented PAKE, RFC 9807) — the MVP has **no account and
  no password**; device pairing is the only auth.

## 8. Do-not-hand-roll inventory

| Need | Use |
|---|---|
| Noise handshake | `snow` (XX→IK) |
| AEAD / ECDH / hash / RNG | `ring` / `aws-lc-rs`, `libsodium` (`dryoc`) |
| TLS / QUIC | `rustls` / `quinn` |
| PAKE (v1.0) | audited OPAQUE (`opaque-ke`) + `argon2` |
| Key storage | OS secure elements (TPM/CNG, Secure Enclave, StrongBox) |
| Signing / provenance | Sigstore/cosign + Rekor (or HSM); SLSA L3 (deferred to pre-"secure"-claim audit) |

All crypto lives in `core/crypto`; it is fuzzed from Phase 0 and budgeted for **one external crypto
audit before any public "secure" claim**.
