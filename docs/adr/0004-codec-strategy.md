# ADR-0004 — Codec strategy: H.264 low-latency High profile + AV1 tier (Phase 4); HEVC excluded

- **Status:** Accepted.
- **Date:** 2026-06-09.
- **Related:** `docs/ARCHITECTURE.md` §2 (media pipeline), `docs/TECH-STACK.md`, `docs/PROJECT-PLAN.md` §10.

## Context

Video codec choice is the product's #1 licensing landmine and a primary latency lever. The host
streams a low-latency desktop over QUIC (ADR-0002); WAN text legibility and glass-to-glass latency
both depend on the encoder configuration, and patent exposure depends on the codec family.

## Decision

- **Primary: H.264, low-latency High profile** (NOT Constrained Baseline — Baseline hurts WAN text
  legibility). Constrained Baseline is offered only to decoders that require it, via capability
  negotiation.
- **Latency config:** zero B-frames, low/zero lookahead, CABAC on, **intra-refresh instead of full
  IDR**, ~1-frame VBV.
- **Hardware encoders** NVENC / QuickSync / AMF / VideoToolbox / MediaCodec, with a **mandatory
  software floor** (x264 / openh264) — VM/passthrough hosts break NVENC, and the GeForce NVENC
  concurrent-session cap constrains multi-session.
- **AV1 quality tier** on capable GPUs (rav1e and HW AV1) is a **Phase-4 line item**, not touched
  in year one.
- **HEVC excluded** (three patent pools; not worth the exposure).

## Consequences

- H.264 carries known royalties — accepted for the MVP as the universally-decodable baseline.
- AV1 is royalty-free and becomes the quality tier later (Phase 4), behind the `VideoEncoder` trait
  so it is additive, not a rewrite.
- The mandatory software floor guarantees the product runs where hardware encode is unavailable.
- Counsel sign-off gates remain: NVIDIA Video Codec SDK redistribution terms and the GeForce NVENC
  session-count cap (see `docs/TECH-STACK.md` "Legal beyond codecs").
