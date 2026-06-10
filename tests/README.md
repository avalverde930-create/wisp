# tests/

Cross-cutting tests that span more than one crate. Unit tests stay inside their crate. This is the home of the concrete **Test & Verification Strategy** (not just empty dirs).

## Strategy (wired into phase exit criteria)
- **(a) Fuzzing (Phase 0+):** cargo-fuzz targets on EVERY wire-protocol decoder and the Noise handshake — the #1 place a packet-facing product gets RCE'd. `core::transport` packet parser + `core::crypto` handshake are designated targets.
- **(b) NAT-traversal matrix (Phase 2):** full-cone/restricted/port-restricted/symmetric/CGNAT/IPv6-only harness emitting the P2P-success-rate SLI (the metric that governs relay cost).
- **(c) Relay abuse (Phase 2):** SSRF to link-local/metadata (169.254.169.254), open-proxy, allocation exhaustion.
- **(d) Cross-encoder/decoder interop (Phase 3):** NVENC<->VideoToolbox<->WebCodecs profile/level negotiation.
- **(e) Soak/chaos (Phase 2-3):** silent relay->direct upgrade; Wi-Fi<->cellular ICE restart; long-session rekey.
- **(f) Input-injection correctness/security (Phase 1-2).**

## Layout (created with its phase)
- `integration/` — cross-crate contract tests (MVP: host<->client over a loopback quinn link).
- `fuzz/` — cargo-fuzz targets (Phase 0+).
- DEFERRED: `e2e/` (Playwright web + native driver harness — Phase 3, when web exists); `load/` (k6/locust relay+signaling soak — Tier 1+, when there is a relay to load-test).
- `fixtures/` — shared test data + MOCK keys. NEVER real secrets.
