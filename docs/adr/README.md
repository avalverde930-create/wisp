# Architecture Decision Records

Append-only. One `.md` per decision, `NNNN-kebab-title.md`, never renumbered. Use `0000-template.md`.

## Index
- 0000 — ADR template
- 0001 — Monorepo over polyrepo + Rust core + **deferred-slot reservation** (the north-star tree is documented here; dirs are created per phase)
- 0002 — Transport: **quinn/QUIC for MVP/v1 native; webrtc-rs in Phase 3 for the browser** (never custom UDP; webrtc-rs is a fuzz/hardening target)
- 0003 — **Noise pattern: XX first contact -> cached IK reconnect -> clean re-pair; XXfallback intentionally NOT used (snow lacks the fallback modifier)**
- 0004 — Codec strategy: H.264 low-latency High profile + AV1 tier (Phase 4), HEVC excluded
- 0005 — Host is outbound-only; relay is blind
- 0006 — Pairing: device-bound secure-element keys + out-of-band SAS; no account/OPAQUE until v1.0
- 0007 — **Relay engine: coturn vs eturnal evaluated co-equal (maintainer bus-factor is a security input)**
- 0008 — **Session-0 secure-desktop helper: its own subcrate + IPC trust-boundary threat model**
- 0009 — **Device-key storage: X25519 vs TPM/CNG NIST curves** — **Accepted Option A** (non-FIPS X25519 + OS-keystore-wrapped); no FIPS for the MVP
- 0010 — **Phase 0/1 is interactive-session only — no virtual display / input driver** (defer the Windows driver-signing track until testing proves it necessary)
