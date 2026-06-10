# Technology Stack

## Language: Rust everywhere security-critical
Core, host agent, backend services, FFI surfaces are Rust. Memory safety on a hostile-network-facing video/packet/crypto workload IS the security thesis; the wire module is shared host<->client<->server.

## FFI surfaces (added per client, not on commit 1)
- **MVP: desktop + host via direct Rust link / cbindgen — the ONLY FFI surface in the MVP.**
- iOS / Android (Phase 3): UniFFI (Swift + Kotlin). Production-grade as of 0.30 (Apr 2026).
- Web (Phase 3): wasm-bindgen. UniFFI's JS/WASM path is aspirational — do NOT use it for web. Browser owns media transport; WASM core owns protocol/crypto handshake.

## Build-vs-leverage
| Capability | Decision | Choice | License / note |
|---|---|---|---|
| Crypto / TLS | Leverage | rustls + aws-lc-rs/ring; libsodium (dryoc); snow (Noise XX->IK); audited OPAQUE (v1.0); argon2 | ISC/MIT/Apache — clean. Never hand-roll crypto. |
| **Native transport (MVP/v1)** | **Leverage** | **quinn (QUIC)** — encryption, CC, datagrams, connection migration; the stable Rust transport story | Apache-2.0/MIT. |
| Browser transport + extra NAT (Phase 3) | Leverage | webrtc-rs (ICE/STUN/TURN/DTLS-SRTP); str0m evaluated co-equal | MIT/Apache. Avoid Google C++ libwebrtc. **Treat as a hardening + fuzz target, version-pinned** (v0.17 feature-frozen w/ ~109 KiB/conn leak; v0.20 sans-io only RC in 2026). Choice in ADR-0002 after a soak spike. |
| Video encode | Leverage HW; build orchestration | NVENC/QSV/AMF/VideoToolbox/MediaCodec; software floor rav1e/openh264/x264 | **Review NVENC SDK redistribution AND the GeForce consumer NVENC session-count cap (multi-session host constraint).** rav1e BSD. |
| Video codec | Decision | **H.264 low-latency High profile + AV1 tier (Phase 4); HEVC excluded** | #1 licensing landmine. AV1 royalty-free; H.264 known royalties; HEVC = 3 pools, excluded. |
| Audio | Leverage | Opus | BSD. |
| Media framework | Build thin | bespoke Rust capture->encode->packetize; avoid FFmpeg/GStreamer in hot path | FFmpeg GPL when --enable-gpl; dynamic-link LGPL only; counsel sign-off. |
| Signaling (Phase 2) | Build | Rust (axum + tokio + tungstenite) | own it. (Go is the defensible alt.) |
| TURN relay (Phase 2) | Leverage | **coturn vs eturnal evaluated co-equal** + Rust cred-minter; STUNner only on K8s | coturn/eturnal BSD. Maintainer bus-factor is a selection input (ADR-0007). |
| Desktop UI | **MVP: winit+wgpu single window; Tauri Phase 3** | proves the pipeline with no webview/IPC/TS | avoid Electron. |
| Mobile UI (Phase 3) | Build thin | SwiftUI + VideoToolbox/Metal; Compose + MediaCodec/Vulkan | native decoders matter. |
| Web client (Phase 3) | Build thin | WASM core + minimal React/TS; WebCodecs + WebRTC | Chromium-family confirmed; Safari/WebKit Encoded-Transform support is a Phase-3 compatibility spike (Open Q #13), not a permanent exclusion. |
| Protocol codegen | Build later | **MVP: hand-written core/wire.** Phase 2: buf+prost. Phase 3: + ts-proto. | teeth at the 2nd consumer. |

## Resolved conflicts
- **Native transport: quinn/QUIC for MVP/v1 (REVERSED from a WebRTC-first draft); webrtc-rs added Phase 3 for the browser. Never custom UDP.** Isolates webrtc-rs's 2026 instability to the one phase that needs it.
- Noise: XX first contact -> cached IK reconnect -> clean SAS re-pair on key change (snow lacks fallback modifier). ADR-0003.
- Codec: H.264 low-latency High profile + AV1 tier (Phase 4), HEVC excluded.
- Capture: WGC primary, DXGI fallback (LocalSystem on secure desktop -> session-0 helper).
- Relay engine: coturn vs eturnal co-equal (ADR-0007).
- Signaling language: Rust (shared wire crate).
- MVP UI/build: winit+wgpu + hand-written wire + no TS toolchain.

## Do NOT fork RustDesk (AGPL-3.0)
Network-served proprietary product on AGPL triggers source disclosure. Reference the hbbs/hbbr topology; write clean-room code.

## Legal beyond codecs (counsel-sign-off gates)
- Crypto export: EAR Cat 5 Pt 2 self-classification/CCATS + OFAC sanctioned-country download block.
- NVIDIA Video Codec SDK redistribution + GeForce NVENC session-count cap.
- GDPR/CCPA as a hosted relay/signaling operator (connection-metadata controller): privacy policy, DPA, retention.
- FTO glance at remote-desktop input-injection/latency patents; trademark care naming competitors.
- Wiretap/two-party-consent law for session recording + assist-others.
