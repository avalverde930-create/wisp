# ADR-0009 — Device-key storage: X25519 vs. TPM/CNG NIST curves (Phase-0 spike)

- **Status:** Accepted (2026-06-09) — **Option A** chosen for the MVP. The Phase-0 spike now only needs to *implement and verify* Option A, not choose among options.
- **Date:** 2026-06-09
- **Deciders:** Owner + crypto-grade `core/crypto` / `core/identity` ownership.
- **Related:** ADR-0003 (Noise pattern), `docs/SECURITY.md`, `docs/security/crypto-spec.md`, `docs/PLATFORM-MATRIX.md`.

## Context

The security design calls for a **non-exportable device-identity static keypair held in the
platform secure element** (Windows **TPM 2.0** via CNG, Apple **Secure Enclave**, Android
**StrongBox**). Separately, the native E2EE handshake is **Noise** via the `snow` crate, which
uses **X25519 (Curve25519)** for the static and ephemeral Diffie-Hellman (ADR-0003:
`Noise_XX_25519_ChaChaPoly_BLAKE2s` → cached `Noise_IK`).

These two requirements may **not compose** on Windows:

- Windows **CNG** and its **FIPS-validated Platform Crypto Provider** expose **NIST ECDH curves
  (P-256 / P-384)** for TPM-backed, non-exportable keys — **not Curve25519 / X25519**.
- Therefore a raw **X25519 static may not be generatable as a non-exportable key *inside* the
  TPM** on Windows. The "non-exportable in hardware" property and the "X25519 Noise static"
  property can pull in opposite directions.

This was surfaced by external review and must be resolved **before the cipher suite and key
hierarchy freeze**, because it determines both the handshake suite and the storage model.

## Decision — **Option A, taken 2026-06-09** (owner call)

**The MVP uses Option A: non-FIPS X25519 + OS-keystore-wrapped device key.** No FIPS for the MVP; FIPS / NIST-curve (Option B) is recorded as a **later enterprise constraint**, revisited only if a customer compliance bar requires it. The Phase-0 spike now *implements and verifies* Option A (generate an X25519 static, wrap it at rest via Windows DPAPI / CNG software KSP, report storage class to the audit layer); it no longer needs to choose among the options below — they are retained for the record.

The options that were on the table:

- **Option A — Non-FIPS X25519, OS-keystore-wrapped key.** Keep the standard Noise X25519 suite.
  The X25519 private key lives in software but is **wrapped at rest by the OS keystore**
  (Windows DPAPI/CNG software KSP, Apple Keychain, Android Keystore). This is *protected* but is
  **not** hardware-generated-non-exportable; the audit log records the storage class as such.
- **Option B — FIPS / NIST-curve handshake.** Move the static-key agreement onto a **NIST curve
  (P-256/384)** so the device key can be a genuine TPM/Secure-Enclave non-exportable key. This
  changes the Noise suite away from 25519 and is heavier; only justified if a FIPS posture is
  required (decided at the same gate).
- **Option C — Hybrid.** TPM-resident NIST key used to *attest/seal* a software-wrapped X25519
  Noise static (TPM guards the wrapping key; Noise still uses X25519). Spike must confirm this
  doesn't merely relocate the exposure.

## Consequences

- Until the spike resolves, **"non-exportable in the TPM" is the design *target*, not a
  guarantee** — `docs/SECURITY.md`, `docs/PROJECT-PLAN.md §4.3`, and `docs/PLATFORM-MATRIX.md`
  all state this caveat, and `core/src/identity.rs` carries the note.
- The decision **co-freezes** with the FIPS gate (a FIPS requirement forces Option B and also
  forces AES-GCM/SHA-2 over ChaCha/BLAKE2 in ADR-0003).
- Whichever option wins, the **software-key degradation rules are unchanged**: non-bypassable
  warning, never for unattended access or first pairing, storage class recorded per device.

## Exit criterion (Phase 0)

A working spike that generates, persists, and uses a device static key under **Option A** (X25519
wrapped at rest by the OS keystore) on Windows 11 Pro, with the storage class correctly reported to
the audit layer, before any handshake code is frozen.
