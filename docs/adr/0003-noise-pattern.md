# 0003. Noise pattern: XX first contact -> cached IK reconnect -> clean re-pair (no XXfallback)

- **Status:** Accepted
- **Date:** 2026-06-09

## Context
The draft specified 'Noise IK with XXfallback (Noise Pipes)'. Verified (June 2026): the `snow` crate tracks the Noise spec but does NOT implement the fallback modifier — XXfallback / Noise Pipes is unbuildable on snow. Hand-rolling a handshake fallback would violate do-not-hand-roll in the single worst place to do it (the handshake).

## Decision
Default to **`Noise_XX_25519_ChaChaPoly_BLAKE2s`** for first contact (full mutual auth, SAS-verifiable). **Cache the peer static key** and use **`Noise_IK`** for subsequent 0-RTT reconnect. On an IK decrypt failure (host static rotated), perform a **CLEAN re-pair** (the SAS model already requires this on key change) — NOT an automatic XXfallback. FIPS note: if FIPS is required (decided Phase 0), substitute AES-GCM/SHA-2 before this freezes.

## Consequences
- We get mutual auth + forward secrecy + fast reconnect using only patterns snow ships.
- A host key rotation forces an explicit, loud re-pair — consistent with the trust model, at the cost of a one-time user step.
- No bespoke handshake code; the crypto attention budget stays on trust model + key lifecycle.
