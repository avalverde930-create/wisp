# Architecture

## 0. North-star vs starting surface
This document describes the TARGET architecture. The repo starts thin (see README). Traits, the versioned proto, the service split, and the client matrix are the destination; Phase 1 is a single-crate vertical slice. Designing the boundary before a concrete impl exists guarantees designing it wrong — so we EXTRACT abstractions from working code, not ahead of it.

## 1. The spine: one core, four stable interfaces (target)
The product is one Rust core consumed by thin platform shells. The core owns everything dangerous (capture-coordination, codec orchestration, transport, crypto, session/consent, protocol). Every per-OS / per-GPU difference hides behind a stable interface so any implementation swaps without touching the others.

- `FrameSource` — GPU texture handles + dirty rects + cursor metadata. Impls: WgcSource, DxgiSource, ScreenCaptureKitSource, PipeWireSource.
- `VideoEncoder` — GPU texture in, encoded access unit + frame type + bitrate-feedback hook out. Impls: Nvenc, Qsv, Amf, VideoToolbox, Software.
- `MediaTransport` — send(packet, reliability, priority), on_receive, on_bandwidth_estimate, on_loss. Impls: **QuicTransport (quinn — MVP/v1 native), WebRtcTransport (Phase 3 browser + extra NAT)**.
- `InputSink` — inject mouse/keyboard/touch/pen. Impls: Win32SendInput, MacCGEvent, LinuxLibeiSink.

GPU texture handles stay opaque behind the interface; zero-copy lives inside each implementation. If NVENC types leak into the transport, shipping AMF becomes a rewrite — that boundary is load-bearing.

## 1a. Sequencing: concrete before trait (the highest-leverage rule)
In Phase 1 write WgcSource, NvencEncoder, the quinn socket, and Win32SendInput as PLAIN STRUCTS in `core/`, end-to-end, until pixels appear on a second machine. Extract the trait at the first moment a SECOND impl forces it (the x264 SoftwareEncoder is the natural first trait-forcing moment for VideoEncoder). The four traits stay documented here as the target; they become code in Phase 1's back half / Phase 2.

## 2. Media pipeline
`Capture -> Encode -> Pace/Packetize -> Transport === network === Transport -> Jitter Buffer -> Decode -> Render`, with input on a reliable reverse channel.
- Capture: WGC primary on Win11, DXGI Desktop Duplication fallback (LocalSystem on the secure desktop -> session-0 helper, Phase 2); ScreenCaptureKit (mac); PipeWire+portal (Linux). Frames stay on GPU. Cursor out-of-band, composited client-side. Dirty-rect hints + idle-suppression heartbeat.
- Codec: **H.264 low-latency High profile, zero B-frames, CABAC on** (NOT Constrained Baseline — Baseline hurts WAN text legibility; Constrained Baseline offered only to decoders that require it via capability negotiation). AV1 quality tier on capable GPUs is a **Phase-4 line item, not touched in year one**; HEVC excluded. Software floor mandatory (VM/passthrough hosts break NVENC; GeForce NVENC session-count cap constrains multi-session).
- Latency-critical: zero B-frames, low/zero lookahead, intra-refresh instead of full IDR, ~1-frame VBV. Budget: 30-60 ms LAN, 60-110 ms good WAN.
- Adaptive bitrate: transport bandwidth-estimate -> encoder target bitrate loop; shed bitrate -> resolution -> fps.
- Audio (Phase 2+): Opus 48 kHz, in-band FEC; client jitter buffer owns A/V sync, biased to minimum latency. (Phase-1 slice is video + input only.)
- Input replay protection: inputs are authenticated inside the session AEAD; on resume after pause/background, queued inputs are dropped (no replay).

## 3. Wire protocol — three channels over one secured connection
1. Media (unreliable, real-time): VIDEO_FRAME, AUDIO_FRAME, CURSOR_UPDATE.
2. Control (reliable, ordered): INPUT_EVENT, DISPLAY_CONFIG, BANDWIDTH_REPORT, QUALITY_MODE, KEYFRAME_REQUEST.
3. Bulk (reliable, consent-gated): CLIPBOARD, FILE_OFFER/CHUNK/ACK. Clipboard/files treated as untrusted input.
**Crypto-envelope invariant: ALL THREE channels ride inside the Noise/SFrame envelope, including bulk. Bulk is E2E AND capability-gated. No optimizer may exempt large file transfers.**
**MVP: hand-written structs in `core/wire`** (no proto/buf). **Phase 2: versioned `proto/wisp/v1/` + buf (prost).** **Phase 3: add ts-proto** when the web/TS client first consumes the protocol. 'Never hand-write types twice' earns teeth at the second consumer.

## 4. Connectivity & NAT model — control plane vs data plane
- **Phase 1: NO control plane.** LAN-only: mDNS or manual IP; direct QUIC; on-screen SAS. This is the zero-server cold-start story.
- Control plane (signaling, Phase 2): brokers SDP/ICE + mints ephemeral TURN creds; never carries media, never holds peer keys, never sees plaintext; horizontally stateless behind Redis. Host holds a persistent OUTBOUND connection — this satisfies 'never expose the host'. **Anti-abuse: auth-before-presence, pairing-attempt lockout, PoW/token on floods, blinded/rotating rendezvous IDs (no raw device-ids on the wire). Mobile push-to-wake (APNs/FCM) for backgrounded clients, Phase 3.**
- Data plane: ICE gathers host/srflx/relay + IPv6 candidates; **outbound** UDP hole-punching + simultaneous-open + symmetric-NAT port prediction; **IPv6-first**; then blind relay. **No router port-mapping (UPnP-IGD / NAT-PMP / PCP):** instructing the router to open an inbound port would violate the "host opens no inbound public port" invariant, so direct connectivity comes from outbound hole-punching and IPv6 only. Relay (coturn/eturnal — decided ADR-0007) forwards only ciphertext, TURN-over-TLS:443, ephemeral HMAC creds, SSRF-hardened (denied-peer-ip for RFC1918/link-local/metadata).
- Reconnection: REGISTERED -> SIGNALING -> CHECKING -> {DIRECT | RELAYED}; background ICE silently upgrades relay->direct; ICE restart (not teardown) on network change. QUIC connection migration survives IP changes natively (a reason quinn is the native data plane from day one).

## 5. Dependency-direction law (documented now, mechanically enforced when the graph grows)
`wire/proto -> core -> {host, client, services, bindings, apps}`. Five rules:
1. core never depends on an app/host/service. 2. apps never import other apps. 3. shared types in exactly one place per language. 4. wire/proto depends on nothing. 5. no lateral core cycles (crypto <- session <- transport).
**Enforcement scales with the graph:** while Rust is the only language, Cargo's compile-time cycle ban + cargo-deny banned edges enforce it for free. The bespoke check-dep-direction.sh + ESLint no-restricted-paths + Turborepo tags + CODEOWNERS arrive in Phase 3 (TS) / at first hire. Exceptions kept under crypto-grade ownership from day one: **core/crypto and core/audit**.
Security-first is in the layout: core/crypto is the only crypto home; core/audit owns the tamper-evident log; relay is blind; api-gateway (Tier 2) is the only internet-facing tier; secrets never enter the tree.

## 6. Backend: three services, three scaling axes (target; Phase 2+)
Signaling (concurrent connections), TURN/Relay (bandwidth/egress), Auth/Identity (durability, v1.0). Never merged in production, but may run as ONE binary behind feature flags at Tier 0. Postgres is the single source of truth, read cached by all. api-gateway deferred to Tier 2 (Caddy + middleware is the Tier-0 edge). The solo-VPS and SaaS-fleet are the same code; only topology changes.
