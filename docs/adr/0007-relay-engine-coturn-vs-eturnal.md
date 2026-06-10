# ADR-0007 — Relay engine: coturn vs. eturnal evaluated co-equal (final pick in Phase 2)

- **Status:** Accepted — co-equal evaluation; final selection recorded here after a Phase-2 soak spike.
- **Date:** 2026-06-09.
- **Related:** `docs/ARCHITECTURE.md` §4, `docs/PROJECT-PLAN.md` §3 & §10, `docs/TECH-STACK.md`, ADR-0005 (relay is blind).

## Context

When direct connectivity fails (CGNAT / symmetric NAT / restrictive firewalls — the default
assumption in 2026, not the edge case), sessions fall back to a **TURN relay**. The relay is the
**most-exposed internet-facing component** and is treated as fully untrusted (ADR-0005: it forwards
ciphertext only). Because it is the highest-exposure surface, **maintainer bus-factor is a security
input**, not just an ops preference.

## Decision

- **Evaluate coturn and eturnal as co-equal candidates** rather than defaulting to coturn. Decide in
  a **Phase-2 soak spike**; record the winner back into this ADR.
  - **coturn:** the well-trodden `use-auth-secret` ephemeral-credential model; but no full-time
    maintainer and a large open-issue backlog — a bus-factor risk on the most-exposed component.
  - **eturnal (ProcessOne):** more actively committed; Erlang/OTP operational profile.
  - **STUNner** only if the deployment is already on Kubernetes.
- **Non-negotiable relay properties** (whichever engine wins): forwards only ciphertext (never a
  decryption point); **TURN-over-TLS on 443** to survive captive-portal/corporate firewalls;
  **ephemeral HMAC credentials** (`use-auth-secret`, minute-scale) — never a static TURN password
  in a client; **SSRF-hardened** (`denied-peer-ip` for RFC1918 / link-local / cloud-metadata
  `169.254.169.254`); per-allocation quotas. A tiny Rust control plane mints the ephemeral creds.

## Consequences

- The relay engine is a recorded, revisitable decision — not an unexamined assumption.
- Either engine is operated as a dumb, blind pipe; switching engines later changes ops, not the
  security model (ADR-0005 holds regardless).
- Relay bandwidth/egress is the cost axis that scales with failed-direct-connect rate, governed by
  the NAT-traversal success-rate SLI (the Phase-2 NAT test matrix).
