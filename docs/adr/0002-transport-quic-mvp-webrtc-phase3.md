# 0002. Transport: quinn/QUIC for MVP/v1 native; webrtc-rs in Phase 3 for the browser

- **Status:** Accepted (reverses the draft's WebRTC-first v1)
- **Date:** 2026-06-09

## Context
The draft made webrtc-rs the v1 data plane. The review verified that in 2026 webrtc-rs v0.17 is feature-frozen with documented ~109 KiB/connection leaks and the sans-io v0.20 is only an RC; str0m's P2P path is under-tested. WebRTC's unique value is (a) a free browser client and (b) ICE/STUN/TURN NAT traversal — NEITHER of which a LAN/native MVP (Phase 1) or even native-only Phase 2 strictly needs first. quinn (QUIC) gives encryption, congestion control, datagrams, and connection migration with a far more stable Rust story.

## Decision
Abstract transport behind a `MediaTransport` trait (extracted from working code, Phase 1 back half). **Ship quinn/QUIC as the MVP/v1 native data plane.** **Add webrtc-rs in Phase 3** for the browser client and as a second `MediaTransport` for extra NAT coverage — treated as a hardening + fuzz target, version-pinned, after a soak bake-off vs str0m (recorded by amending this ADR). Browsers keep WebRTC permanently (no P2P QUIC in the sandbox). End-to-end encryption sits ABOVE transport (Noise / SFrame). **Never custom UDP.**

## Consequences
- The security-critical v1 data plane rides the stable quinn stack; webrtc-rs's 2026 churn is isolated to the phase that needs a browser.
- Adding webrtc-rs is additive (new MediaTransport impl), not a re-architecture.
- The browser client is structurally slower than native QUIC — communicated in UX and handled by capability negotiation.
