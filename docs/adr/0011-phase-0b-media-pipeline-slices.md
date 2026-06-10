# ADR-0011 — Phase 0b media pipeline lands as ordered, independently-verifiable slices

- **Status:** Accepted (2026-06-10) — owner call.
- **Date:** 2026-06-10.
- **Related:** ADR-0004 (codec strategy), ADR-0010 (interactive-session capture, no driver),
  `docs/ROADMAP.md` (Phase 0b), `docs/RUNNING-THE-SPIKE.md`.

## Context

Phase 0a proved the loop with **GDI full-frame capture → LZ4 → QUIC/Noise → softbuffer
render**. It tops out around ~20–23 fps at 1536×864 on this machine, bottlenecked by
full-frame software work (BitBlt + GetDIBits + LZ4 of ~5 MB every frame) and full-frame wire
bytes (~165–183 KiB/frame). Phase 0b's goal is **30–60 fps at < 50 ms glass-to-glass** via:

1. **WGC capture** (Windows.Graphics.Capture) → frames as D3D11 textures (GPU-resident,
   dirty-region aware, correct for occlusion/multi-monitor).
2. **Hardware H.264** (NVENC / QSV / AMF, x264 software floor) consuming those textures.
3. **wgpu GPU render** on the client (texture upload + YUV→RGB), replacing softbuffer.

That is an entire subsystem, not one change. Landing it as a single commit would mean a large
pile of WinRT/D3D11/Media-Foundation interop merged **unverified** — the opposite of the
verify-each-step discipline used through Phase 1 (DPAPI, IK reconnect each shipped green).

## Decision

**Phase 0b is delivered as four ordered slices, each built, gated (test/clippy/fmt/release),
and bench-verified before commit.** Each slice stands alone and does not regress the loop.

- **0b/4a — Interframe codec seam (this slice).** Introduce a *stateful* `FrameEncoder` /
  `FrameDecoder` in `core::codec` with a GOP cadence: emit a **keyframe** (full LZ4 frame) at
  GOP boundaries and on any dimension change, else an **XOR delta** vs the previous frame, then
  LZ4. On a mostly-static desktop the delta is overwhelmingly zeros, so wire bytes collapse.
  Pure Rust, fully unit-testable + bench-verifiable. **Crucially, the encoder-state seam (prev
  frame + GOP/keyframe/resync) is the same shape H.264 needs**, so 4c slots a hardware encoder
  in behind the same `FrameEncoder`/`FrameDecoder` boundary; only the payload format changes.
- **0b/4b — WGC capture → D3D11 texture.** Replace the GDI grab with a WGC
  `Direct3D11CaptureFramePool` (free-threaded) on a D3D11 device; copy the texture to CPU to
  feed the existing codec for now. **Runtime fallback to GDI** if WGC init fails (no regression).
  This stands up the D3D11 device + GPU-resident frame that 4c consumes.
- **0b/4c — Hardware H.264 behind the `FrameEncoder` seam.** A `VideoEncoder` impl using a
  hardware H.264 MFT (NVENC/QSV/AMF) with an x264/software floor and capability negotiation in
  HELLO (ADR-0004). Wire `FrameCodec::HwH264 = 3`. Decoder via Media Foundation / hardware.
- **0b/4d — wgpu render.** Replace softbuffer with a wgpu present path (texture upload +
  colour-convert), the precondition for zero-copy decode→present.

Slices may reorder if testing demands (e.g. if a hardware H.264 MFT is unavailable on the
target box, the x264 floor lands first), but each still ships verified and independently.

## Consequences

- **No large unverified native merge.** Every commit keeps the loop runnable and green; risk
  is bounded per slice. If a native slice (4b/4c) cannot be verified on a given machine, the
  fallback path keeps Phase-0a behaviour and the slice is reported honestly rather than faked.
- **4a is not throwaway.** The XOR-delta *payload* is provisional, but the stateful
  encoder/decoder seam, the GOP/keyframe/resync logic, and the host-capture/client-decode
  threading it introduces are exactly what the H.264 impl reuses.
- **Bandwidth win arrives early.** 4a alone cuts static-desktop wire bytes by ~10–100× (the
  WAN-relevant metric) before any hardware codec exists.
- Documentation (`RUNNING-THE-SPIKE.md`, ROADMAP) tracks which slices have landed so the
  "works now vs deferred" table never drifts from the code (the Phase-1 drift this avoids).
