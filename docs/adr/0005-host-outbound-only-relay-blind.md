# ADR-0005 — Host is outbound-only; the relay is blind

- **Status:** Accepted (the core security invariant; non-negotiable).
- **Date:** 2026-06-09.
- **Related:** `docs/SECURITY.md` (Invariants 1 & 3), `docs/ARCHITECTURE.md` §4, `docs/security/threat-model.md`.

## Context

The product's central security promise is *hostile-network-by-default*: a port-scan of the host
must reveal nothing, and the rendezvous/relay infrastructure must be untrusted even when we operate
it. Incumbents that expose an inbound port or terminate plaintext at a relay have repeatedly been
breached.

## Decision

- **The host opens no inbound public port, ever.** From Phase 2 it holds a persistent **outbound**
  connection to the signaling service; that single decision satisfies "never expose the host."
- **NAT traversal is outbound-only:** ICE hole-punching (+ simultaneous-open, symmetric-NAT port
  prediction) + **IPv6-first**, then blind relay. **No router port-mapping (UPnP-IGD / NAT-PMP /
  PCP)** — instructing the router to open an inbound port would re-introduce the exact surface this
  invariant removes.
- **The relay is blind:** it forwards only ciphertext and is never a decryption point; E2E keys
  live only in the two endpoints' secure elements. TURN-over-TLS:443, ephemeral HMAC creds,
  SSRF-hardened (see ADR-0007).

## Consequences

- Direct connectivity is won by outbound hole-punching and IPv6, never a port-forward — so CGNAT /
  symmetric-NAT cases fall back to the (blind) relay rather than a router config the host can't
  safely make.
- No "expose to internet" toggle is ever shipped (it would become the downgrade vector).
- The signaling/relay split (ADR-0007, Phase 2) inherits this: signaling brokers introductions but
  never carries media or sees plaintext.
