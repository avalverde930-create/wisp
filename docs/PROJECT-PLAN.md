# Secure Remote Desktop — Master Project Plan (Final, Build-Ready)

> **Repo home:** `03 TECHNOLOGY/Secure Remote Desktop/` — a root-level technology domain, deliberately **outside** the `01 LEGAL/` perimeter. This codebase holds no client-sensitive legal data; the DMS folder rules and the `YYYY-MM-DD` file-naming convention do **not** apply (source trees are exempt per root `CLAUDE.md`). All paths in this repo are relative to the repo root; never assume a drive letter.

This plan integrates an adversarial four-lens review panel (security, scalability, MVP-pragmatism, completeness) into the draft. Two of the panel's highest-stakes factual claims were independently verified as current for June 2026: (1) the `snow` Noise crate still does **not** implement the fallback modifier, so "Noise Pipes / IK-with-XXfallback" is **unbuildable as drafted**; (2) Azure Trusted Signing's **2026-03-26 intermediate-CA rotation reintroduced SmartScreen "unrecognized app" warnings** until per-binary reputation accrues. Both findings reshaped the plan below.

**The governing tension the panel surfaced, and how this plan resolves it:** the original draft is an excellent *v1.0 architecture document* and a poor *solo-MVP plan*. The fix is not to weaken the architecture — the spine (one Rust core, four traits, versioned protocol, three-service split, dependency-direction DAG) is correct and is kept verbatim — but to **separate the north-star target from the day-one starting surface**. The architecture is the contract; the first phase is ruthlessly thin. Roughly 60–70% of the original tree is *deferred* (reserved as a documented slot, not scaffolded as an empty README-bearing directory) until the phase that forces it. Nothing good is deleted; everything premature is sequenced.

Where specialists or reviewers disagreed, the chosen direction is stated explicitly here and in *§10 Resolved Conflicts*.

---

## 1. Vision & Differentiation

**Product.** A security-first remote-desktop product that lets the owner securely **view and control their own PC** from another device (phone, tablet, laptop, browser), and later extend — with explicit, time-boxed, logged consent — to assisting other machines. Near-term host: **Windows 11 Pro**. Long-term: hosts on Windows/macOS/Linux; clients on desktop, iOS, Android, and web.

**The wedge no incumbent owns.** The market splits into four camps — consumer-IT support (TeamViewer, AnyDesk), low-latency pixel-streaming (Parsec, Moonlight+Sunshine), zero-friction Google-account access (Chrome Remote Desktop), and self-hostable open source (RustDesk). Each leaves the same lane open: a product that is **private-by-default (zero-knowledge E2E with device-pinned keys), optionally fully self-hostable, AND professionally secure-by-default with the code-signing discipline, audited supply chain, and published threat model that RustDesk lacks and that the 2024 AnyDesk cert-theft and TeamViewer APT29 breaches proved the incumbents also lack.**

**Positioning vs Duet (the named target).** Duet's remote desktop is a convenience add-on bolted onto a second-display app, with undocumented cloud-mediated security and no self-host. We are not in Duet's category and Duet cannot pivot into ours without abandoning its creative/second-display identity.

**Tagline shape.** *"Remote-control your machines on hostile networks, with keys only you hold — self-host it or let us, your choice."*

**Four ownable promises:**
1. **Hostile-network-by-default.** The host never opens an inbound port; it dials *out* to a rendezvous broker. A port-scan of the host shows nothing. No Google account in the path.
2. **Zero-knowledge / own-your-infrastructure.** End-to-end encryption the relay cannot read; one-command secure self-host with secure defaults (no cleartext keys, no test certs, client allowlist on).
3. **Audited & transparent.** Reproducible builds, HSM/Sigstore signing, published threat model, third-party audit before any "secure" claim — the opposite of Duet/AnyDesk opacity and the direct answer to RustDesk's audit gap.
4. **Consent-first assist.** The scam-tainted "assist others" use case reclaimed with time-boxed, explicitly-granted, fully-logged sessions.

**Two non-negotiables that gate every decision below:** (1) **Safe & secure above all** — never naively expose the host; assume hostile networks; the relay is the enemy. (2) **Scalable from day one** — one shared core, thin platform shells, stateless control/data planes, so a solo build grows into a product without re-architecting. *Per the panel: "scalable from day one" is satisfied by the architecture **document** plus a few load-bearing seams (the shared wire module, the Noise channel, the outbound-only host), NOT by instantiating every directory on commit 1.*

---

## 2. Scope

> **Resolved Open Question #1 (de-risking the MVP):** the **day-one dogfood client is the desktop client**, not iOS/web. Rationale (completeness + MVP lenses): a desktop client has no app-store gatekeeper, full capture/inject capability, and no WebKit/Insertable-Streams limitation. iOS is **client-only forever** (no third-party background screen-capture entitlement makes an iOS *host* infeasible) and is deferred to Phase 3 behind store-review analysis. The owner can still override which *desktop* OS is the daily driver.

### MVP (v0.1) — "Securely view + control my own Windows 11 PC from one desktop client, over my own LAN"

> **Scope boundary (do not overclaim):** v0.1 is **LAN-only**. Secure remote access across the internet / hostile *wide-area* networks (signaling + NAT traversal + relay) is the **Phase-2** deliverable, not the MVP. The MVP's always-on E2E encryption still treats the **local** network as hostile (any LAN peer is untrusted), but v0.1 does **not** provide internet remote access.
- Windows 11 Pro host agent: WGC capture (DXGI Desktop Duplication fallback), hardware H.264 encode (software floor), input injection — **in the user's interactive session only**.
- **One** desktop dogfood client (the OS the owner uses daily), sharing the Rust networking/crypto core.
- Zero-knowledge pairing: device-pinned keypairs, out-of-band SAS-verified pairing code/QR, host approves each new device. **No account, no password — device pairing is the only auth in the MVP.**
- E2E AEAD on every packet (**Noise XX first contact → cached Noise IK reconnect**; see §4.2). Never expose RDP/raw ports.
- **Transport for the MVP is QUIC (quinn)** — encryption, congestion control, datagrams, connection migration — *not* webrtc-rs (deferred to Phase 3 with the browser client; see §10).
- Input + clipboard + single-monitor video + audio. Latency visibly competitive with CRD, aspiring toward Parsec.
- Secure-by-default everything: no naked ports, **signed binaries via Azure Trusted Signing from day one** (with explicit SmartScreen-reputation caveat, §6.4), host audit log of every connection, deny-by-default capabilities (view-only until escalated).
- **Explicit MVP limitation, documented in-product:** cannot view or control the UAC/secure-desktop/lock screen — any elevated window silently halts control. The session-0 helper that fixes this is a Phase-2 deliverable (§4.3, §8).

**Deferred OUT of the MVP (panel over-engineering findings — reserved, not deleted):** OPAQUE account login (→ v1.0 accounts); per-frame SFrame ratchet (→ ships with the browser/relay-forwarding path, Phase 3); `services/api-gateway` as a distinct tier (→ Tier 2); cross-device audit-log anchoring (→ v1.0, needs a 2nd device); FIDO2 attestation + capability *tokens* in the schema (→ `srd/v2`); the full TS/pnpm/Turborepo toolchain (→ Phase 3, app #2); buf dual-language codegen (→ when a 2nd language consumes the protocol); the 7-crate `core/` split, `cargo-hakari`, and the bespoke `check-dep-direction.sh` gate (→ when the workspace and a second consumer make them earn their keep).

### v1.0 — "A real product"
- Self-hosting bundle: one `docker-compose` deploy of signaling + coturn/eturnal relay, secure defaults, identical binaries to hosted, server-key pinning.
- Browser client over **WebRTC** (DPI-resilient TURN-over-TLS:443; QUIC express-lane is native-only). **Now** SFrame/Insertable-Streams E2EE ships (Chromium-family; Safari gated — see §4.2 and Open Q #13).
- Multi-monitor, file transfer, dynamic adaptive bitrate, AV1 quality tier on capable GPUs.
- iOS + Android **clients** (Rust core via UniFFI). iOS push-to-wake (APNs) and Android (FCM) for backgrounded clients (§3.4).
- macOS + Linux host agents behind the capture/encode abstraction.
- Account layer with **OPAQUE** login; device trust-store sync; org/team RBAC, central policy, opt-in session recording.
- Account/device **recovery** (offline enrollment recovery code) — designed alongside the key hierarchy, not bolted on.
- Compliance posture: reproducible builds, HSM/Sigstore signing, third-party pen-test, SOC 2 Type 2 program kicked off **when a customer's procurement asks** (not front-loaded).

### Later
- AV1 hardware-encode maturity (RTX 40+/Arc/RDNA4 dual encoders), 4K/120, HDR.
- Wake-on-LAN / remote power, remote print, USB redirection.
- MSP/fleet management, SSO/SCIM, SIEM export.
- FIDO2 device attestation, TPM/Secure-Enclave key binding, just-in-time brokered ephemeral enterprise sessions (`srd/v2` schema).

---

## 3. System Architecture

### 3.1 The architectural spine: one core, four stable interfaces (the north-star target)
The product is **one Rust core** consumed by thin platform shells. The core owns everything dangerous (capture-coordination, codec orchestration, transport, crypto, session/consent state, protocol). Every per-OS and per-GPU difference hides behind a **stable internal interface** so any implementation swaps without touching the others — this is what wins "scale without re-architecting":

- **`FrameSource`** — yields GPU texture handles + dirty rects + timestamp + cursor metadata. Impls: `WgcSource`, `DxgiSource`, `ScreenCaptureKitSource`, `PipeWireSource`.
- **`VideoEncoder`** — accepts a GPU texture, emits an encoded access unit + frame type + bitrate-feedback hook. Impls: `NvencEncoder`, `QsvEncoder`, `AmfEncoder`, `VideoToolboxEncoder`, `SoftwareEncoder`.
- **`MediaTransport`** — `send(packet, reliability, priority)`, `on_receive`, `on_bandwidth_estimate`, `on_loss`. Impls: `QuicTransport` (quinn — **the MVP/v1 native data plane**), `WebRtcTransport` (Phase 3, browser + extra NAT coverage).
- **`InputSink`** — injects mouse/keyboard/touch/pen. Impls: `Win32SendInput`, `MacCGEvent`, `LinuxLibeiSink`.

GPU texture handles stay opaque behind the interface; zero-copy lives *inside* each implementation. If NVENC types leak into the transport, shipping AMF becomes a rewrite — that boundary is load-bearing.

> **Sequencing correction (MVP lens — the single highest-leverage change in this revision):** *do not write the four traits as code before a single concrete implementation exists.* In Phase 1 the team writes `WgcSource`, `NvencEncoder`, the quinn socket, and `Win32SendInput` as **plain structs in one crate**, end-to-end, until pixels appear on a second machine. The trait is **extracted from working code** at the first moment a second implementation forces it (the `SoftwareEncoder` x264 floor is the natural first trait-forcing moment for `VideoEncoder`). Designing the boundary first guarantees designing it wrong — precisely the NVENC-leaks-into-transport failure the spine warns about. The traits remain the documented target in `ARCHITECTURE.md`; they become code in Phase 1's back half / Phase 2.

### 3.2 Media pipeline (capture → encode → transport → decode → render → input)
```
[Capture] -> [Encode] -> [Pace/Packetize] -> [Transport] === hostile network ===
   host side                                            client side
=== [Transport] -> [Jitter Buffer] -> [Decode] -> [Render]
[Input capture on client] ---- reliable control channel ---- [Input inject on host]  (reverse path)
```

- **Capture (Win11 primary):** Windows.Graphics.Capture (WGC) — cross-GPU, no injection, per-window/background, optional border. DXGI Desktop Duplication is the documented fallback for secure-desktop/RDP gaps (note: DXGI Duplication on the secure desktop requires LocalSystem — this is the session-0 helper's job, §4.3). macOS: ScreenCaptureKit. Linux: PipeWire + xdg-desktop-portal (compositor-brokered consent). Keep frames as GPU textures; never read back to CPU. Capture the hardware cursor out-of-band and composite client-side.
- **Dirty-rect / idle suppression:** feed WGC/DXGI dirty+move rects to the encoder as region hints; on zero dirty rects send a few-byte "frame identical" heartbeat, not an encoded frame (battery/bandwidth win on a static desktop).
- **Codec (negotiated at session start):** **H.264 — low-latency High profile, zero B-frames, CABAC on** as the universal baseline (*not* Constrained Baseline; Baseline lacks CABAC/B-frames and hurts text legibility at a given bitrate over WAN — Constrained Baseline is offered only to decoders that require it, via capability negotiation). Every decoder, all browsers, the WebRTC path; known MPEG-LA/Via-LA royalties (budget line). **AV1** as the opt-in quality/efficiency tier on capable GPUs (NVENC AV1 on Ada/Blackwell, Intel Arc/QSV, AMD RDNA3+) — royalty-free, ~30–50% better compression, but adds ~2–3 frames latency so it is gated behind a max-quality/bandwidth-constrained mode, never the low-latency default, and is a **Phase-4 line item — not touched in year one.** **HEVC/H.265 is excluded from shipped binaries** until a patent-pool license is deliberately acquired. Software floor: openh264 / x264 (ultrafast,zerolatency) / rav1e/SVT-AV1 — mandatory for GPU-passthrough/VM hosts where hardware NVENC breaks.
- **Latency-critical encoder settings (all vendors):** zero B-frames, low/zero lookahead, **intra-refresh instead of periodic full IDR** (kills the keyframe latency spike on lossy links), CBR/capped-VBR with a ~1-frame VBV/HRD buffer, infinite GOP.
- **Audio:** WASAPI loopback (Win) / ScreenCaptureKit (mac) / PipeWire (Linux) → Opus 48 kHz, 10–20 ms, in-band FEC. Stamp A/V with a common monotonic capture clock; the client jitter buffer owns alignment and biases toward **minimum latency over perfect lip-sync** (desktop work is input-latency-dominant). *Audio is a Phase-2+ refinement; the Phase-1 thin slice is video + input only.*
- **Adaptive bitrate:** the closed loop *transport bandwidth-estimate → encoder target bitrate* (every ~100–200 ms) is the whole game. On congestion, shed **bitrate → resolution → frame rate** in that order (sharp text > fluidity for desktop). Use temporal scalability to drop frames without an IDR; recover loss with a forced intra-refresh region + NACK within budget.
- **Glass-to-glass latency budget:** **30–60 ms LAN, 60–110 ms good WAN.** The jitter buffer is the single biggest tunable; expose a "responsiveness vs smoothness" slider.
- **Input (reverse path):** `SendInput`/`InjectSyntheticPointerInput` (Win), `CGEventPost` (mac), libei via RemoteDesktop portal (Wayland). Input rides a **reliable, ordered** control channel — never the lossy media path. Sequence-numbered + timestamped for the client cursor predictor. **Replay protection (gap closed):** inputs are authenticated inside the session AEAD; on session resume after pause/background, queued inputs are dropped — a backgrounded session cannot replay buffered input on reconnect.

### 3.3 Wire protocol (three logical channels over one secured connection)
1. **Media (unreliable, real-time):** `VIDEO_FRAME{stream_id, frame_seq, capture_ts, frame_type, is_duplicate, dirty_rect_count, payload}`, `AUDIO_FRAME`, `CURSOR_UPDATE`.
2. **Control (reliable, ordered):** `INPUT_EVENT`, `DISPLAY_CONFIG`, `BANDWIDTH_REPORT`, `QUALITY_MODE`, `KEYFRAME_REQUEST`, `STAT_PING`.
3. **Bulk (reliable, flow-controlled, consent-gated):** `CLIPBOARD`, `FILE_OFFER/CHUNK/ACK` with resumption. Clipboard/files are treated as untrusted input (exfil/injection vector).

**Crypto-envelope invariant (gap closed):** **all three channels ride inside the Noise (native) / SFrame (browser) envelope** — including the bulk clipboard/file channel. No future optimizer may exempt large file transfers "for performance." Bulk is *both* E2E-encrypted *and* capability-gated.

**Session lifecycle:** `HELLO{proto_ver, device_id, capabilities[codecs, hw, max_res, av1?]}` → mutual auth (device pairing + per-session keys, never a static shared secret) → `NEGOTIATE{codec, resolution, fps, quality_mode}` → media flows. **Capability negotiation, never assumption** — this is what lets H.264/AV1/future codecs and features coexist without versioning the whole protocol.

> **Protocol-evolution discipline (MVP lens — gap closed):** during Phase 0–1 the wire format changes daily; a rigid `buf` breaking-change gate fights spike velocity. So: **Phase 0–1 hand-write the wire structs as a plain Rust module (`core/wire`)** shared by host and client via a normal Cargo path dependency — no `proto/`, no `buf`, no codegen. The versioned `proto/` + `buf` pipeline + breaking-change gate is introduced **at the start of Phase 2** (first stable protocol) and becomes *strict* only when a **second language** (the web/TS client, Phase 3) consumes it. The "version on first commit / never hand-write types twice" principle is honored the moment it has teeth (two consumers), not before.

### 3.4 Connectivity & NAT model — control plane and data plane kept separate
```
                    ┌─────────────────────────┐
                    │  SIGNALING (control)    │   outbound TLS from BOTH sides
   HOST  ───────────┤  rendezvous + presence  ├───────────  CLIENT
   (Win11) outbound │  brokers SDP/ICE only   │ outbound    (phone/laptop/web)
            │        └─────────────────────────┘        │
            │        ╔═════════════════════╗            │
            └────────╢ DATA PLANE          ╟────────────┘
                     ║ 1. direct P2P (UDP) ║  preferred (free)
                     ║ 2. TURN relay       ║  fallback, E2E ciphertext only
                     ╚═════════════════════╝
```
- **Phase 1 has NO control plane.** LAN-only: mDNS discovery or manual IP entry; direct QUIC connection; SAS pairing on-screen. The signaling/rendezvous server is a **Phase-2** deliverable. This is the cold-start story (gap closed): the first pairing happens with zero servers.
- **Control plane (signaling/rendezvous, Phase 2):** low-bandwidth, always-on, knows *who* is online and brokers *introductions* (SDP offer/answer + trickled ICE candidates) and mints **ephemeral TURN credentials**. Never carries pixels/input, never holds long-term peer keys, never sees plaintext. Horizontally **stateless** behind a presence store (Redis) with cross-instance fan-out (NATS at scale). The host holds a persistent **outbound** connection here — that single decision satisfies "never expose the host."
  - **Anti-abuse / anti-enumeration (gap closed):** auth-before-presence; rate-limit pairing attempts with lockout; proof-of-work or token gate on connection floods; **blinded/rotating rendezvous identifiers** so a party who learns a `device_id` cannot confirm online/offline status or harvest the allowlist (raw long-term device-ids never go on the wire).
  - **Mobile push-to-wake (gap closed):** a backgrounded iOS/Android client needs APNs/FCM to re-establish; this is a signaling-plane requirement with its own metadata-privacy implication (the push provider learns connection timing) — designed in Phase 3 with the mobile clients.
- **Data plane (the session):** **ICE** gathers host/srflx/relay + **IPv6** candidates; **outbound** UDP hole-punching with simultaneous-open; symmetric-NAT port prediction; then blind relay. **No router port-mapping (UPnP-IGD / NAT-PMP / PCP):** asking the router to open an inbound port would violate the "host opens no inbound public port" invariant, so direct connectivity is won by *outbound* hole-punching and IPv6 only — never a port-forward. **IPv6-first** (no NAT → near-100% direct → no relay cost). **CGNAT is the default assumption, not the edge case** in 2026; plan for relay on IPv4-only mobile.
- **Relay (TURN):** stateless per-allocation, horizontally scalable, **forwards only ciphertext** (a dumb pipe — never a decryption point). TURN-over-TLS on **443** to survive captive-portal/corporate firewalls. Ephemeral HMAC credentials (`use-auth-secret`, minute-scale), **never** a static TURN password in any client. Hardened against open-proxy/SSRF abuse (`denied-peer-ip` for RFC1918/link-local/metadata `169.254.169.254`, per-allocation quotas). **Relay engine choice is a recorded Phase-2 decision, not an assumption:** evaluate **coturn vs eturnal as co-equal** (coturn's `use-auth-secret` model is well-trodden but it has no full-time maintainer / 343 open issues; eturnal/ProcessOne is more actively committed) — this is the most-exposed internet component, so maintainer bus-factor is a security input. Recorded in ADR-0007.
- **Reconnection state machine:** `REGISTERED → SIGNALING → CHECKING → {DIRECT | RELAYED}`, with background ICE that **silently upgrades** a relayed session to direct when a path opens (the biggest perceived-quality win), and **ICE restart over existing session keys** (not teardown) on Wi-Fi↔cellular handoff. QUIC connection migration survives IP changes natively (a reason quinn is the native data plane from day one).

---

## 4. Security Architecture & Threat Model

**Organizing principle:** the relay is the enemy, the network is the enemy, and a stolen device is a question of *when*. Compromise of any single component — relay, network, account server, or one paired device — must not compromise the host.

**Solo-builder discipline (security lens, load-bearing):** over-built security is not wasted effort — it is *dangerous*, because a solo builder who spreads crypto-review attention across ten mechanisms audits none to depth. **Fewer, deeper > more, shallower.** The MVP ships exactly **three** security mechanisms done correctly: (1) the Noise channel, (2) SAS pairing + secure-element device keys, (3) the outbound-only host + local audit log. Everything else (OPAQUE, SFrame, attestation, mTLS, cross-device anchoring, capability tokens) is deferred to the phase that needs it.

### 4.1 Assets & adversaries
- **Assets (priority order):** live screen/audio + input channel (crown jewel) · host device-identity key · client device-identity keys · the paired-devices allowlist · account credentials/session tokens · file-transfer/clipboard contents · audit-log integrity · the update/signing pipeline (highest leverage).
- **Adversaries:** active on-path network MITM · malicious/compromised relay (assume fully hostile) · stolen/lost device · brute-force/credential attacker · supply-chain attacker · malicious peer / over-broad consent.
- **Explicitly out of scope:** an OS/kernel-level-compromised host (but we must never be the vector), nation-state hardware implants, rubber-hose.

### 4.2 The "relay is blind" invariant + two-transport E2EE
- **Native clients (MVP/v1):** Noise Protocol Framework. **Buildable construction (fixed):** default to **`Noise_XX_25519_ChaChaPoly_BLAKE2s`** for first contact (full mutual auth + SAS), **cache the peer static key**, and use **`Noise_IK`** for subsequent 0-RTT reconnect. On an IK decrypt failure (host static rotated), do a **clean re-pair** — *not* an automatic XXfallback. **Why:** the `snow` crate does **not** implement the fallback modifier, so "IK with XXfallback / Noise Pipes" is unbuildable, and hand-rolling a handshake fallback is forbidden (it would violate do-not-hand-roll in the worst possible place). Recorded in **ADR-0003**. This is the WireGuard/libp2p-style construction minus the one piece snow lacks; the SAS re-pair on key change is something the trust model already requires.
- **Browser client (Phase 3 / v1.0):** WebRTC with **DTLS 1.3** transport **plus a mandatory second E2EE layer** via Encoded-Transform/Insertable-Streams + **SFrame**, keyed from a Noise/ECDH agreement over the data channel (not from DTLS) — so a TURN relay/SFU forwards only opaque frames. **If a browser lacks Insertable-Streams support, refuse the session** rather than silently downgrade. **This rule keeps SFrame native-client-free in the MVP** — native 1:1 P2P is already blinded by the Noise tunnel, so per-frame SFrame is *only* needed for the browser/relay-forwarding (SFU-style) and future multi-party "assist" cases. **Consequence (Open Q #13, spike-gated):** WebKit/Safari Encoded-Transform support is **at-risk and unverified**, so "refuse rather than downgrade" *may* mean no web client on Safari/iOS-web — but this is resolved by a **Phase-3 browser-compatibility spike, not a permanent exclusion**. Chromium-family is the confirmed target; Safari/iOS-web ships iff the spike confirms a working Encoded Transform + SFrame path, otherwise iOS is served by the native UniFFI client. (WebRTC Encoded Transforms are now broadly specified and WebKit shipped Safari-18 encoded-transform fixes — hence re-test at Phase 3, don't assume dead.) See §11 Open Q #13 and the Phase-3 exit criterion in §8.

### 4.3 Identity, pairing, authn/authz
- **Device identity = a static keypair held in / wrapped by the secure element:** Windows **TPM 2.0** (CNG/Platform Crypto Provider + DPAPI, VBS/Credential Guard where present); Apple **Secure Enclave**; Android **StrongBox**/TEE. **Open composition risk (Phase-0 spike, ADR-0009):** CNG/TPM (and its FIPS Platform Crypto Provider) expose **NIST curves (P-256/384), not Curve25519**, whereas Noise/`snow` uses **X25519** — so a raw X25519 static may not be generatable as a non-exportable key *inside* the TPM. **DECIDED (2026-06-09, ADR-0009): Option (a)** — non-FIPS X25519 with the key **wrapped at rest by the OS keystore** (not hardware-non-exportable). Option (b), a FIPS/NIST-curve handshake, is a later enterprise constraint. So "non-exportable in the TPM" is explicitly **not** an MVP guarantee — the MVP ships software-wrapped X25519 with the storage class recorded. Detect capability; degrade to software-protected keys only with an explicit user-visible warning. **Hardened degradation (security lens):** the warning is **non-bypassable for unattended access AND for the very first device pairing**, and the audit log records **key-storage class (hardware vs software) per device**, so a later software-key device cannot silently weaken the trust set.
- **Pairing ceremony (defeats pairing-time MITM):** run Noise XX, derive a **Short Authentication String** (numeric/emoji bound to the full handshake transcript), owner compares **out-of-band** (same screen / in-person QR / voice). **No blind trust-on-first-use.** Pin the peer key thereafter; a changed key triggers a loud warning and forces re-pair.
- **Account login — DEFERRED to v1.0:** **OPAQUE (augmented PAKE, opaque-ke / RFC 9807)** is real and sound but solves *password* login, a problem the MVP designs away with device-pinned keys + SAS + no-account-to-reach-your-own-machine. It ships **with accounts/teams in v1.0**, not Phase 1–2. **Local at-rest passphrase: Argon2id** (OWASP-2025 params) is kept for the MVP.
- **Key hierarchy:** secure-element device key → per-session ephemeral (Noise XX/IK, forward-secret) → (browser only) per-frame SFrame ratchet. Rekey within long sessions. **Revocation from day one:** any device revocable from any other; propagated as a **signed, monotonically-versioned** trust-list update; endpoints reject older versions than last-seen. **Honest limitation (risk surfaced):** until the Phase-2 signaling presence layer exists, a fully-P2P MVP has no rendezvous to push a revocation to an *offline* device — **revocation is best-effort until Phase 2**; state this in product and docs.
- **Recovery (gap closed — moved into the design now, even if implemented later):** with non-exportable secure-element keys + zero-knowledge servers, losing the only paired device = permanent host lockout. Reserve the design space **now** (a slot in `core/identity` and the wire schema) for an **enrollment-time offline recovery code** so it is a designed, non-backdoor path — not a late bolt-on that becomes the backdoor the threat model forbids. Implemented in Phase 2 with the key hierarchy frozen; the recovery slot is reserved before the hierarchy freezes.

### 4.4 Authorization, consent, audit
- **Deny-by-default capability model**, not a binary "connected": `VIEW` / `CONTROL` / `CLIPBOARD` / `FILE_TRANSFER` / `AUDIO` / `MULTI_MONITOR` as separate grants; default **view-only**; every escalation is a separate explicit grant. The grant table is designed now with `expires_at` / `consent_proof` for the future "assist others" case so it never needs migration. **(Note: the *grant table* is in `srd/v1`; *capability tokens* and *FIDO2 attestation* are deferred to `srd/v2` to avoid freezing a half-specified security schema.)**
- **Explicit, scoped, time-boxed consent** with a **non-spoofable in-session indicator** (OS-controlled tray + persistent banner) showing who is connected, which capabilities are live, and a **one-click kill switch** (+ global hotkey). Unattended access off by default, gated by the secure element.
- **Append-only, hash-chained audit log** on the host (session start/stop, peer key fingerprint + **key-storage class**, capabilities, file transfers, failed/declined attempts, revocations). **Kept strictly local — never shipped to relay or cloud** (also satisfies the LEGAL-perimeter constraint should the tool ever reach a host holding client data). **Module home (gap closed):** the audit log is a named security asset with an integrity model (hash-chain) easy to get subtly wrong — it gets a dedicated module **`core/audit`**, CODEOWNERS-gated alongside `core/crypto`, not an afterthought field on a session struct. **Cross-device anchoring** of periodic log-head hashes to the owner's other devices is **deferred to v1.0** (it needs a second paired device + sync channel the MVP doesn't have; it is security theater until then).

### 4.5 Secure defaults (ship like this on day one)
1. Host opens **no inbound public port, ever** (outbound-only; no UPnP-IGD / NAT-PMP / PCP / router port-mapping / auto-forward / "expose to internet" toggle; NAT traversal is outbound-only — hole-punching + IPv6 + blind relay). 2. Deny-by-default permissions (view-only). 3. Pairing requires out-of-band SAS. 4. Unattended access OFF. 5. **E2EE always on, non-optional** (no "compatibility/unencrypted" mode — that switch becomes the downgrade attack). 6. Relay treated as untrusted even when we operate it. 7. Local secrets behind Argon2id + secure element (OPAQUE only when accounts exist). 8. Persistent in-session indicator + kill switch. 9. Auto-update verifies signature + version monotonicity **+ pinned TLS fetch channel** before applying. 10. Telemetry off by default; if added, never screen/keystroke/file content.

### 4.6 Do-not-hand-roll
Compose vetted libraries; engineering effort goes to the trust model, defaults, key lifecycle, and consent UX. Handshake → **snow** (Noise; XX→IK as above). AEAD/ECDH/hash/RNG → **ring/aws-lc-rs** + **libsodium (dryoc)**. TLS/QUIC → **rustls / quinn**. PAKE (v1.0) → audited **OPAQUE** + **argon2**. Key storage → OS secure elements. Memory-safe core → **Rust**. Signing/provenance → **Sigstore/cosign + Rekor**, SLSA L3 (deferred to pre-"secure"-claim), reproducible builds, SBOM.

### 4.7 The session-0 / secure-desktop helper — its own trust boundary (architecture + security)
Capturing/injecting on the UAC/lock/login screen requires a **session-0 Windows Service** (LocalSystem, for DXGI Duplication on the secure desktop) **plus an active-session helper**. This is the #1 "works until a prompt appears" killer. **Two corrections:**
- **It is a Phase-2 deliverable, not Phase-1 mandatory** (MVP lens): it is a signed, privileged, session-crossing service easily worth 3–6 solo weeks; treating it as MVP-blocking stalls the whole product. Phase 1 runs purely in the interactive session and **documents the gap in-product**.
- **It is its own subcrate with its own ADR** (scalability lens): the elevated session-0 service is a **separately-signed, separately-privileged binary** whose IPC channel to the user-session agent **is a trust boundary** (a compromised helper is a SYSTEM-level system-wide keystroke injector). It lives in **`host/host-windows-helper`** distinct from `host/host-windows`, with its own mini threat model in **ADR-0008** — drawn *before* the code, so the privilege/IPC boundary is not retrofitted onto a monolith.

### 4.8 Build, signing & secure updates
Reproducible/hermetic builds targeting **SLSA Build L3** (the L3 *attestation* is deferred to the pre-"secure"-claim audit, Phase 4; cargo-audit + SBOM ship from Phase 0). Sigstore/cosign keyless signing (Rekor transparency log) or HSM-held keys — **never a signing key on a dev laptop**. Pin GitHub Actions by **commit SHA, not tag** (the 2025 GhostAction lesson). Ship an SBOM. Client verifies signature **+ monotonic version counter** (rollback protection) **+ OS-native code signature** (Authenticode/notarization/APK v3+) **+ pinned-TLS fetch channel** (the distribution channel is untrusted, distinct from the signing pipeline) before applying any update. A **roll-forward-mandatory / revoke-bad-version** mechanism (not just rollback protection) is part of the update design.

### 4.9 Capture of sensitive surfaces (gap closed)
WGC will happily capture password managers, OS secure-input fields, and DRM-protected windows. A security-first product **surfaces that it does** and **honors per-window capture-exclusion flags** (`SetWindowDisplayAffinity`/`WDA_EXCLUDEFROMCAPTURE` semantics) where the OS provides them — documented in the platform matrix and the in-session indicator.

---

## 5. Technology Stack

**Language: Rust for the entire core, host agent, backend services, and FFI surfaces.** Memory safety on a hostile-network-facing video/packet/crypto workload *is* the security thesis; the best transport/crypto crates are Rust-native; Rust compiles to every named target; one language means the wire module is literally shared across host, client, and server. RustDesk proved a Rust remote-desktop core ships.

**Shared-core reuse — FFI surfaces (added only as each client is built, not on commit 1):**

| Target | Mechanism | Phase | Why |
|---|---|---|---|
| Desktop client + host (Win/mac/Linux) | Direct Rust link / cbindgen C ABI | **MVP** | No FFI seam when the shell is Rust. **This is the only FFI surface in the MVP.** |
| iOS / Android | **UniFFI** (Swift + Kotlin) | Phase 3 | Production-grade for Swift/Kotlin as of 0.30 (Apr 2026); auto-generates idiomatic bindings. |
| Web | **wasm-bindgen** | Phase 3 | The proven web path. **UniFFI's JS/WASM path is aspirational — do not use it for web.** Browser owns media transport (sandbox can't do raw UDP/ICE); the WASM core owns protocol/crypto handshake. |

**Build-vs-leverage:**

| Capability | Decision | Choice | License note |
|---|---|---|---|
| Crypto / TLS | **Leverage** | rustls + aws-lc-rs/ring; libsodium via dryoc; snow (Noise XX→IK); audited OPAQUE (v1.0); argon2 | ISC/MIT/Apache — clean, commercial-safe. **Never hand-roll crypto.** |
| **Native transport (MVP/v1)** | **Leverage** | **quinn (QUIC)** — encryption, congestion control, datagrams, connection migration; the stable Rust transport story | Apache-2.0/MIT — clean. |
| Browser transport + extra NAT (Phase 3) | **Leverage** | **webrtc-rs (ICE/STUN/TURN/DTLS-SRTP)** for browser + as a second `MediaTransport`; str0m evaluated co-equal | MIT/Apache. **Avoid Google C++ libwebrtc.** Treat as a **hardening + fuzz target**, version-pinned, not a settled black box (large in-process DTLS/SCTP/ICE surface; webrtc-rs v0.17 feature-frozen w/ ~109 KiB/conn leak, v0.20 sans-io only RC in 2026). Choice recorded in ADR-0002 after a Phase-0/Phase-2 soak spike. |
| Video encode | **Leverage HW; build orchestration** | NVENC/QSV/AMF/VideoToolbox/MediaCodec; software floor rav1e/openh264/x264 | **Review NVENC SDK redistribution terms AND the GeForce consumer NVENC session-count cap** (historically ~3–5 simultaneous sessions) — a real multi-monitor/multi-session host constraint, not a footnote. rav1e BSD. |
| Video codec | **Decision** | **H.264 low-latency High profile + AV1 quality tier (Phase 4); HEVC excluded** | **#1 licensing landmine.** AV1 royalty-free; H.264 known royalties (budget line); HEVC = 3 pools, excluded. |
| Audio codec | **Leverage** | Opus | BSD — clean. |
| Media framework | **Build thin** | Bespoke Rust capture→encode→packetize; avoid FFmpeg/GStreamer in the hot path | FFmpeg LGPL-by-default but **GPL when `--enable-gpl`**; GStreamer plugin patent traps. Dynamic-link LGPL only, never static-link GPL. Counsel sign-off before any such dep lands. |
| Signaling/rendezvous (Phase 2) | **Build** | Rust (axum + tokio + tungstenite, or quinn control channel) | Your security core — own it. *(Go is the defensible alternative.)* |
| TURN relay (Phase 2) | **Leverage** | **coturn vs eturnal evaluated co-equal** + tiny Rust control plane minting ephemeral HMAC creds; STUNner only if on K8s | coturn/eturnal BSD — clean. Maintainer bus-factor is a selection input (ADR-0007). |
| Desktop UI | **Leverage (Phase 3); minimal in MVP** | **MVP: a single `winit` + `wgpu` window** (raw blit / egui) — proves the pipeline with no webview/IPC/TS. **Tauri introduced in Phase 3** for the polished UI. | Avoid Electron. |
| Mobile UI (Phase 3) | **Build thin** | SwiftUI + VideoToolbox/Metal; Jetpack Compose + MediaCodec/Vulkan | Native decoders matter for battery/latency — don't use Flutter. |
| Web client (Phase 3) | **Build thin** | WASM core + minimal React/TS; WebCodecs + WebRTC | First-class but feature-reduced (H.264, slower than native QUIC); Chromium-family confirmed — Safari/WebKit Encoded-Transform support is a Phase-3 compatibility spike (Open Q #13), not a permanent exclusion. |
| Protocol codegen | **Build later** | **MVP: hand-written `core/wire` Rust module.** Phase 2: `buf`+prost (Rust). Phase 3: add ts-proto when the TS client exists. | "Never hand-write types twice" earns teeth at the 2nd consumer, not before. |

**Do NOT fork RustDesk.** It is **AGPL-3.0** — a network-served proprietary product built on it triggers source-disclosure. Use it as a *reference architecture* (hbbs/hbbr rendezvous+relay shape); write clean-room code.

---

## 6. Infrastructure & DevOps

### 6.1 Three backend services, three scaling axes (the target; introduced in Phase 2)
| Service | Job | Scales on | State | If it dies |
|---|---|---|---|---|
| **Signaling** | Broker SDP/ICE; presence; pairing | concurrent connections | ephemeral (Redis); identity in Postgres | active P2P sessions keep running; new connects fail — degraded, not down |
| **TURN/Relay** | Relay ciphertext when P2P blocked | **bandwidth (egress $$$)** | per-allocation, node-local | that relay's sessions reconnect elsewhere |
| **Auth/Identity** (v1.0) | Identity, device enrollment, grant-based authZ, entitlements, billing | requests/sec (light), durability-critical | Postgres (source of truth) | new logins fail; existing sessions ride cached short-lived tokens |

A relay bandwidth spike must never knock over login; a signaling thundering-herd must never touch billing. **Postgres is the single source of truth.** These contracts are the *design target*; the **MVP has zero backend services** (LAN-only). **`api-gateway` is deferred to Tier 2** — at Tier 0 the edge role is **Caddy + per-service middleware**, not a distinct internet-facing service to misconfigure. **At Tier 0 the three services can even run as one binary behind feature flags and split at deploy time** without changing the source layout.

### 6.2 Growth path (no re-architecture)
- **Tier 0 (solo/self-host, ~$25–50/mo):** ONE VPS, Docker Compose — Caddy (TLS) + signaling + (v1.0) identity + coturn/eturnal + Postgres + Redis. Nightly `pg_dump` → restic → cheap object storage. A real, shippable product. **Only `infra/docker` (the Compose bundle) exists at first.**
- **Tier 1 (first paying users):** pull Postgres to managed (PITR/failover); split the relay onto its own bandwidth-sized box(es); managed Redis. **IaC (Terraform/OpenTofu) starts here, not on commit 1.**
- **Tier 2 (SaaS, multi-region):** N stateless signaling nodes behind an LB + Redis pub/sub; regional relay fleet (GeoDNS/Anycast); 2+ stateless auth nodes; introduce `api-gateway`. Move to a **managed container platform (Fly.io/Cloud Run/ECS) before Kubernetes** — adopt K8s only when ops toil justifies it. **Never start on K8s** (so `infra/k8s` and `infra/ansible` do not exist until Tier 2 — their mere presence on day one is an attractive nuisance).

### 6.3 Cost lever
**Relay egress is the dominant, most volatile cost.** Treat the **P2P-vs-relay ratio as a first-class SLI**. Invest in good ICE (STUN, hole-punching, **IPv6-first**, TURN-TLS:443 as last resort), put relays near users. **Who pays:** hosted tier meters relayed-GB as a paid feature; self-hosters pay their own VPS egress (the honest reason privacy-conscious owners self-host). Cloudflare Realtime TURN (first 1 TB/mo free) is a sane zero-ops starting/overflow relay.

### 6.4 CI/CD & signing
GitHub Actions, **path-filtered matrix**; server services → multi-stage distroless images → GHCR by SHA. **SBOM (Syft), Trivy/Grype + gitleaks/trufflehog on every PR; cargo-audit from Phase 0.** **GitHub OIDC federation** — no long-lived secrets in Actions. **CI security gates that FAIL the build:** any plaintext private key or TURN `static-auth-secret`; any compile of the TURN data plane with a static-credential path enabled (the plan says "never a static TURN password in any client" — CI *enforces* it).

**Windows signing — corrected to verified 2026 reality.** Use **Azure Trusted Signing** (GA, ~$10/mo, no EV issuance) as the app-signing default. **Do NOT claim it "kills SmartScreen":** Microsoft's **2026-03-26 intermediate-CA rotation** (to *Microsoft ID Verified CS AOC CA 03 / EOC CA 04*) reintroduced "Windows protected your PC" warnings on freshly-signed binaries until per-hash reputation accrues (weeks; hundreds of clean installs). A brand-new security product has **zero reputation**, so **early adopters WILL see SmartScreen friction** regardless of valid signing. Budget either (a) an **EV cert via Azure Key Vault for instant reputation at launch** (note: EV no longer *bypasses* SmartScreen since 2024, but accrues reputation faster), or (b) an explicit "expect SmartScreen friction for early adopters" UX note — do not train users to click through warnings, which trains them out of the product's core security habit. **Driver signing** (a virtual display/input driver) is a **separate** Windows Hardware Dev Center track with months of lead time — resolve Open Q #7 **early** because it gates the session-0 helper sizing. macOS: `codesign` → `notarytool` → `stapler` (+ Gatekeeper for the non-store self-host build). Mobile: Play App Signing / App Store Connect API.

### 6.5 Secrets, observability & client diagnostics
**Secrets:** SOPS+age (Tier 0, encrypted-in-repo, decrypt-on-host) → Infisical/Vault/cloud KMS (Tier 1+); JWT-signing key + TURN `static-auth-secret` rotate on a schedule; the device-enrollment CA key is the crown jewel (HSM/KMS at scale). **No real secret ever in the repo unencrypted, in a log, or in an error message — CI-enforced (§6.4).**
**Observability:** **in-process structured tracing (`tracing`) is enough for the MVP**; the OpenTelemetry → Grafana collector stack is introduced **in Phase 2** when there are multiple services to correlate across. SLIs that matter: connection-success rate, **P2P-vs-relay ratio**, time-to-first-frame, relay egress GB/region, auth p99. Propagate a session ID through auth → signaling → TURN-cred mint.
**Client diagnostics (gap closed):** "can't connect" is the dominant support load. Ship an **opt-in, privacy-safe "export diagnostic bundle"** (connection logs, ICE candidate types, codec negotiation — **never** screen/keystroke/file content) plus optional symbolicated crash reporting (Sentry-style) with a documented privacy config. The MVP has *no* client telemetry path otherwise.

---

## 7. Repository Organization — and why it scales

**Decision: a single polyglot MONOREPO.** The wire protocol and Rust core are shared by host, every client, and the backend. A protocol change and all its consumers land in **one atomic, compiler-verified PR** — eliminating the #1 remote-desktop failure mode (host/client protocol drift). One clone, one CI, one `just bootstrap`.

**Defer the scaffolding; reserve the slots in ADRs (the central revision to the tree).** *An empty directory with a README is a liability, not scaffolding.* On **commit 1** create ONLY what Phase 0–1 touches:

```
core/          ONE Rust crate (modules: wire, crypto, transport, session, media, identity, audit)
host/          host-core (lib) + host-windows (bin)
client/        the single desktop dogfood client (winit + wgpu)
spikes/        throwaway Phase-0 de-risking experiments
tools/         dev scripts (bootstrap, ci helpers)
docs/          ARCHITECTURE.md, SECURITY.md, ROADMAP.md, TECH-STACK.md, adr/, security/, platform matrix
infra/         docker/ only (Compose bundle is Phase 2, but the dir reserves the slot)
tests/         cross-cutting integration tests (unit tests stay in-crate)
```

Everything else from the north-star tree — `proto/`, the 7-crate `core/*` split, `services/*`, `bindings/*`, `clients-native/*`, `apps/*` (Tauri/web/site), `packages/*`, `crates/workspace-hack`, `infra/{terraform,k8s,ansible}`, `tools/release`, `tests/{e2e,load}` — is **documented as a reserved slot in ADR-0001 + CONTRIBUTING.md** and created **only when its phase arrives**. This removes ~20 empty README-bearing dirs and an entire TS build system from the solo-start critical path while preserving the architectural map.

**The single-crate `core/` is split into separate crates only when forced** (compile times hurt, or a second binary needs a subset). The 7-crate split, `cargo-hakari`/`workspace-hack`, and the bespoke `check-dep-direction.sh` graph gate are **Phase 2–3** items — Cargo's compile-time cycle ban + `cargo-deny` banned edges already enforce the Rust DAG for free while Rust is the only language. CODEOWNERS, ESLint `no-restricted-paths`, and Turborepo tags are **team governance re-introduced at the first-hire / v1.0 boundary** — friction with zero benefit for a solo dev still discovering the structure. The exceptions kept under CODEOWNERS-grade ownership from the start: **`core/crypto` and `core/audit`** (the two integrity-critical modules).

**The dependency-direction law (the heart of "scalable") — documented now, mechanically enforced when the graph grows:**
```
proto/ or core/wire (depends on NOTHING)  →  core  →  {host, client, services, bindings, apps}
```
1. `core` never depends on an app/host/service (leaf-ward only). 2. Apps never import other apps. 3. Shared types live in exactly ONE place per language. 4. The wire schema is the single source of truth. 5. No lateral core cycles (`crypto` ← `session` ← `transport`, never reverse). **Security-first is baked into the layout:** `core/crypto` is the ONLY home for crypto primitives (audited + fuzzed as a unit); `core/audit` owns the tamper-evident log; the relay is designed blind (ADR-0005); `api-gateway` (Tier 2) is the single internet-facing tier; secrets never enter the tree; seeded ADRs record every trust-boundary decision before any code.

---

## 8. Phased Roadmap

> **Solo-developer calendar reality (gap closed — blunt line so the roadmap isn't mistaken for a quarter of work):** Phase 0 spike ≈ **2–4 weeks**. Phase 1 thin LAN slice ≈ **4–6 weeks** solo. Phase 2 secure-internet ≈ **2–4 months** solo. Do **not** start Phase 3 multi-platform until Phase 2 has been dogfooded for weeks. **Prerequisite to even exercise Phase 1: two physical machines (or a 2nd GPU/VM) on the same LAN.** First spike of all: stand up the Windows graphics dev environment (Media Foundation / NVENC SDK / D3D11 interop / cross-process GPU texture sharing) — itself a real onboarding tax.

**Build the hard, scary part first — as a running artifact, not an abstraction.** The biggest morale/learning risk for a solo build is going months with nothing that runs. So Phase 0 and Phase 1 are **one sequenced vertical slice**, not parallel paperwork.

### Phase 0 — One vertical de-risking spike (throwaway) + repo skeleton
Replace the four "parallel spikes with one-page notes each" with **ONE sequenced vertical spike that becomes Phase 1**, because latency/capture/transport only mean anything measured end-to-end: **WGC capture → HW H.264 encode → quinn loopback → decode → present → SendInput**, on one machine, measuring glass-to-glass latency (<50 ms LAN target). Keep **NAT traversal as a genuinely separate Phase-2 spike** (the only risk that doesn't compose into a LAN slice). Also: the **transport soak spike** — confirm quinn for native and *note* (do not yet build) the webrtc-rs-vs-str0m bake-off for Phase 3. Deliver: minimal skeleton (`core/`, `host/`, `client/`, `spikes/`, `docs/adr/`), threat model v0, CI security gates (cargo-audit, SBOM, secret scan), Azure Trusted Signing wired up, ADR-0001/0002/0003.
**FIPS + device-key-storage gate — DECIDED 2026-06-09 (ADR-0009):** **no FIPS for the MVP** — keep ChaCha20-Poly1305 / BLAKE2s (not AES-GCM/SHA-2) — and **device keys use Option A** (non-FIPS X25519 wrapped at rest by the OS keystore, not TPM-non-exportable). FIPS / NIST-curve is a later enterprise constraint. (Was Open Q #5; the Phase-0 spike now implements-and-verifies Option A rather than choosing.)
**Exit:** vertical spike presents a live frame + moves the mouse on loopback; <50 ms measured; protocol/trait *target* reviewed (not yet coded); threat model v0 approved; FIPS decided; CI green.

### Phase 1 — LAN-only MVP (one Windows host ↔ one desktop client)
A SINGLE host binary + SINGLE client binary, ONE crate, same-LAN, **mDNS or manual-IP**, WGC primary-monitor capture → HW H.264 (x264 software fallback) → **plain Noise(XX)-encrypted quinn** → `winit`+`wgpu` render + `SendInput`. Hand-written wire structs (`core/wire`). Explicit owner approval to start; **SAS pairing code compared on both screens every session**; non-spoofable "you are being viewed/controlled" indicator + one-click/hotkey kill switch; clear **"cannot control elevated prompt / UAC / lock screen — known limitation"** banner. No proto/buf, no Tauri, no services, no traits-as-code yet.
**Definition of done a solo dev can hold in their head:** *"On two machines on my LAN, I run host.exe on PC-A and client.exe on PC-B, enter PC-A's IP, compare a 6-digit pairing code on both screens and accept, then see PC-A's primary monitor at ~1080p30 and control its mouse/keyboard, with all bytes Noise-encrypted and a banner on PC-A showing the live session + a kill key. UAC/lock-screen explicitly not supported yet."* Everything not required by that sentence is deferred.
**Exit:** that DoD met; reconnect after a Wi-Fi blip (QUIC migration); keyboard+mouse+scroll; pairing required every session; no unauthenticated path can start a session; signed installer (SmartScreen caveat documented).

### Phase 2 — Secure remote over the internet (the security-defining phase)
**Now** the versioned `proto/` + `buf` (Rust/prost) replaces `core/wire`; traits are **extracted from the working Phase-1 code**. Zero-knowledge **signaling** (first public endpoint; relays handshake material it cannot read; stateless, **rate-limited, auth-before-presence, blinded rendezvous IDs**). **ICE/STUN/UDP hole-punching** + three-tier fallback (direct → IPv6 → relay). **coturn/eturnal relay** (decided in ADR-0007) forwarding ciphertext only, ephemeral HMAC creds, TURN-TLS:443, SSRF/open-proxy hardened. Full **E2E crypto + SAS pairing** hardened: device-bound keys, per-session forward secrecy (XX→IK), mutual auth, **revocation** (best-effort→server-pushed), **recovery code** design implemented. Host hardening: default-deny inbound, outbound-only, paired-only, `core/audit` hash-chained log. **The session-0 `host-windows-helper`** (secure-desktop capture/inject) with ADR-0008 IPC-boundary threat model. Connection-quality adaptation (bitrate/resolution scaling, FEC/retransmit). OTel + first deploy (Docker Compose, Tier 0). **NAT-traversal test matrix harness** (full-cone/restricted/port-restricted/symmetric/CGNAT/IPv6-only) emitting the P2P-success-rate SLI; **relay abuse tests** (SSRF to metadata IP, open-proxy, allocation exhaustion); **wire-parser + handshake cargo-fuzz targets** wired into CI.
**Exit:** direct-connect measured across ≥3 network types, relay covers the rest; all traffic E2E with relay/signaling provably unable to decrypt; no inbound port on the host; revocation + recovery work; reconnects survive IP changes; secure-desktop helper passes its IPC threat-model review; external/self crypto+pairing review completed; fuzz targets green.

### Phase 3 — Multi-platform clients + host portability groundwork
Client shells over the Rust core via **UniFFI** (iOS SwiftUI / Android Compose) and **wasm-bindgen + webrtc-rs** (web). **Now** the TS toolchain (pnpm/Turborepo/ts-proto/`packages/*`), **Tauri** desktop UI, the `check-dep-direction.sh` gate, and `cargo-hakari` are introduced — they earn their keep at the second language / second app. Browser E2EE via **SFrame/Insertable-Streams** (Chromium-family). Mobile **push-to-wake** (APNs/FCM). Add the webrtc-rs `MediaTransport` (after the soak bake-off vs str0m). Host portability spikes: macOS ScreenCaptureKit + accessibility (TCC); Linux PipeWire/portal + libei. File transfer + clipboard (consent-gated, inside the crypto envelope), multi-monitor, HiDPI. **Cross-encoder/decoder interop matrix** (NVENC↔VideoToolbox↔WebCodecs profile/level).
**Exit:** ≥3 new client platforms ship view+control parity; **web works on Chromium-family browsers; Safari/iOS-web is gated on a WebKit Encoded-Transform compatibility spike (Open Q #13) — supported iff it passes, else served by the native iOS client**; one non-Windows host reaches Phase-1 capability behind a flag; no client weakens the crypto/pairing invariants.

### Phase 4 — Polish, scale, optional SaaS
AV1 where available, adaptive-bitrate maturity, GPU-decode clients, 4K/120. Signaling+relay autoscaling, SLOs, runbooks, automated cert rotation. Optional SaaS: organizations, RBAC, device inventory, exportable audit logs, signed offline self-host licenses, **OPAQUE account login**, billing. **Consent-based assist-others** (separate, time-boxed, explicitly-granted, auto-expiring, architecturally distinct). Hardening: third-party pen-test, bug bounty, **SLSA L3 attestation**, SOC 2 Type 2 program (when procurement asks). Cross-device audit anchoring; FIDO2 attestation (`srd/v2`).
**Exit:** documented SLOs met under load; relay/signaling scale horizontally; signed auto-updates; external pen test passed with no open criticals; self-host path documented and working; assist-others shipped behind explicit opt-in.

---

## 9. Top Risks (cross-cutting)
1. **Security credibility is binary** — one breach/stolen cert ends the brand. → HSM/Sigstore signing, reproducible builds, third-party audit before any "secure" claim, bug bounty, isolate ALL crypto in `core/crypto` and **fuzz it from Phase 0**.
2. **Solo-builder crypto-attention dilution** — ten mechanisms cannot all be self-reviewed to depth. → MVP ships exactly three (Noise, SAS+secure-element, outbound-only+audit-log); everything else deferred to its phase.
3. **NAT traversal reliability is the #1 churn driver** — CGNAT/symmetric-NAT defeats hole-punching. → disproportionate early ICE investment, IPv6-first, outbound hole-punching + simultaneous-open + port prediction (**no router port-mapping — it would break the no-inbound-port invariant**), robust stateless relay from Phase 2, the NAT test matrix as the governing SLI.
4. **Windows secure desktop (UAC/lock screen)** — the #1 "works until a prompt appears" failure, and the session-0 helper is a 3–6-week sub-project. → Phase-1 documents the gap; Phase-2 builds the signed helper as its own subcrate with an IPC threat model.
5. **Transport dependency instability** — webrtc-rs v0.17 feature-frozen w/ memory leak, v0.20 sans-io only RC in 2026. → **quinn is the native MVP/v1 data plane**, isolating webrtc-rs risk to the Phase-3 browser client; the trait makes it additive; version-pinned + fuzzed.
6. **Unbuildable crypto as specified** — `snow` lacks the fallback modifier. → XX→cached-IX construction (ADR-0003), never hand-roll the handshake.
7. **Windows signing / SmartScreen friction** — the 2026-03 CA rotation reintroduced warnings; new products start at zero reputation. → sign from day one, but set UX expectations and consider an EV cert for launch reputation.
8. **GPU-passthrough/VM hosts break NVENC** → the software encoder floor is mandatory, not optional. **GeForce NVENC session-count cap** constrains multi-session hosts.
9. **Codec/AGPL/GPL/export licensing** → AV1-tier + H.264 baseline, exclude HEVC, clean-room (no RustDesk fork), dynamic-link LGPL only; **EAR Cat 5 Pt 2 crypto export classification + sanctioned-country download block**; NVENC redistribution terms; GDPR-as-relay-operator; counsel sign-off before any codec/media dep ships.
10. **Store gatekeepers** — App Store/Play actively scrutinize remote-control apps; an iOS *host* is infeasible. → desktop is the day-one dogfood; store-review analysis precedes any mobile/web client.
11. **Premature K8s/microservice/toolchain sprawl** vs monolithic coupling — hold both lines: three services (deferred to Phase 2), lightest possible deployment, no empty scaffolding.
12. **Solo-maintainer bus-factor** — hosted outage = users offline. → P2P-persistence already gives "your sessions survive our outage"; make it an explicit, tested property; self-host as a first-class tier.

---

## 10. Resolved Conflicts (where specialists / reviewers disagreed)
- **Native transport (WebRTC vs QUIC vs WebRTC-now-QUIC-later):** **Reversed from the draft.** Ship **quinn/QUIC as the MVP/v1 native data plane** (stable Rust story; encryption + CC + datagrams + connection migration; Phase 1–2 ship only native clients so no browser is needed yet), abstract behind `MediaTransport`, and add **webrtc-rs in Phase 3** for the browser client + extra NAT coverage. This also resolves the draft's own ADR-0002 tension (WebRTC's unique value is the free browser client + NAT traversal, neither needed for a LAN/native MVP) and isolates webrtc-rs's 2026 instability to the one phase that requires it. **Never custom UDP.**
- **Noise construction (Noise Pipes/XXfallback vs buildable):** **`Noise_XX` first contact → cached `Noise_IK` reconnect → clean SAS re-pair on key change.** `snow` cannot build XXfallback; hand-rolling a handshake fallback is forbidden (ADR-0003).
- **Browser E2EE / Safari:** keep **"refuse rather than downgrade"** (correct), but treat Safari support as **spike-gated, not excluded**: WebKit Encoded-Transform support is uncertain (WebRTC Encoded Transforms are now broadly specified; WebKit shipped Safari-18 fixes), so a **Phase-3 compatibility spike** decides it. Chromium-family is the confirmed target; Safari/iOS-web ships iff the spike passes, else iOS uses the native UniFFI client. Open Q #13.
- **Codec:** **H.264 low-latency High profile** (not Constrained Baseline) + AV1 tier (Phase 4), **HEVC excluded**.
- **Capture API:** **WGC primary, DXGI Desktop Duplication documented fallback** (LocalSystem for the secure desktop → session-0 helper).
- **Relay engine:** **coturn vs eturnal evaluated co-equal** in Phase 2 (maintainer bus-factor is a security input), recorded in ADR-0007 — not a defaulted assumption.
- **Signaling language:** **Rust** (shared wire crate). Go noted as the defensible alternative.
- **Repo instantiation:** **monorepo as target; ~60–70% deferred as ADR-reserved slots, not empty dirs.** Single-crate `core/` until forced to split.
- **MVP UI/build:** **`winit`+`wgpu` single window + hand-written wire + no TS toolchain** for the MVP; Tauri/proto/buf/Turborepo all Phase 2–3.

---

## 11. Open Questions & Decisions Owed by the Owner
1. ~~Primary near-term client device?~~ **Resolved: the desktop client is the day-one dogfood** (no store gatekeeper, full capture/inject, no WebKit limitation). Owner may still pick *which desktop OS* is the daily driver.
2. **Monetization model:** self-host-free + paid-managed (RustDesk-style), or paid-product-with-self-host-option? Determines whether relay/signaling is a cost center or a product tier.
3. **GPU-equipped host assumption** vs graceful software-encode degradation for GPU-less Windows machines?
4. **Latency ambition:** Parsec-class (~7–10 ms LAN) vs RustDesk-class pragmatism (~18–30 ms LAN)? Shapes Phase-0 transport tuning.
5. **Compliance bar of earliest users** (prosumer vs SMB needing SOC 2/HIPAA)? **DECIDED (2026-06-09, ADR-0009): no FIPS for the MVP** — keep ChaCha20-Poly1305/BLAKE2s + X25519 (device-key **Option A**, OS-keystore-wrapped). FIPS (AES-GCM/SHA-2 + NIST curves) is a later enterprise constraint, revisited only if a customer compliance bar requires it.
6. **Assist-others audience** (family IT vs support desks)? Anti-abuse/logging design differs.
7. **Virtual display/input driver?** **DECIDED (2026-06-09, ADR-0010): NO driver in Phase 0** — interactive-session capture + `SendInput` only. The Windows driver-signing track (months of lead time) is deferred until real testing proves a virtual display/input driver is necessary.
8. **Account-level trust-store sync across the owner's devices in v1**, or strictly per-device pairing? Account sync needs its own E2EE design.
9. **Recovery if ALL paired devices are lost** — design the enrollment-time offline recovery code **before the key hierarchy freezes** (Phase 2), reserve the slot now.
10. **Self-host as a first-class customer tier** (multi-tenant DB + license tooling) vs solo-dev only?
11. **Primary cloud for Tier 2** (AWS/GCP/Azure/Fly/Hetzner) — swappable given the stateless design.
12. **Realtime media-control channel encoding:** stay all-Protobuf, or Cap'n Proto/FlatBuffers for the zero-copy hot path? Benchmark before committing; **all-Protobuf is the safer single-parser surface to start.**
13. **(New) Is a Safari/iOS web client a v1 requirement?** If yes, the SFrame-or-refuse invariant must be re-tested against current WebKit — **Encoded-Transform support on WebKit is uncertain and is resolved by a Phase-3 compatibility spike, not assumed absent** (WebRTC Encoded Transforms are now broadly specified; WebKit shipped Safari-18 fixes). Default answer: Chromium-family web is confirmed; Safari/iOS-web ships iff the spike passes, otherwise iOS uses the native UniFFI client.

---

## 12. Distribution & Platform Gatekeepers (new section — completeness gap)
- **iOS/macOS App Store:** actively scrutinizes remote-control apps (e.g., the Aug-2025 Screens rejection over data-collection clarification; longstanding ban on store-like UI in mirroring apps). **An iOS *host* is infeasible** — there is no third-party background screen-capture entitlement; iOS is **client-only**, with ReplayKit/background-execution limits. State this in §2 Scope.
- **Google Play:** has repeatedly purged apps using `AccessibilityService` for remote control — the Android client's input path must comply or use sanctioned APIs.
- **macOS self-host build:** notarization + Gatekeeper for the non-store distribution.
- **Microsoft Store vs direct signed download:** decision pending; direct download is the likely default (with the SmartScreen-reputation caveat, §6.4).
- **Fallback distribution:** direct download / TestFlight / enterprise if a store rejects. **Choosing the desktop client as the day-one dogfood sidesteps all store risk for the MVP.**

## 13. Test & Verification Strategy (new section — completeness gap)
Concrete deliverables, wired into phase exit criteria (not just tree directories):
- **(a) Fuzzing** — cargo-fuzz/libFuzzer targets on **every wire-protocol decoder** and the **Noise/SFrame handshake**, in CI from Phase 0 (the #1 place a packet-facing product gets RCE'd). `core/transport` packet-parsing path is a designated fuzz target from its first commit.
- **(b) NAT-traversal test matrix** — full-cone / restricted / port-restricted / symmetric / CGNAT / IPv6-only harness emitting the **P2P-success-rate SLI** (the metric that governs relay cost). Phase 2.
- **(c) Relay abuse tests** — SSRF to link-local/metadata (`169.254.169.254`), open-proxy, allocation exhaustion. Phase 2.
- **(d) Cross-encoder/decoder interop matrix** — NVENC↔VideoToolbox↔WebCodecs profile/level negotiation (classic green-screen/artifact field bug). Phase 3.
- **(e) Soak/chaos** — silent relay→direct upgrade; Wi-Fi↔cellular ICE restart; long-session rekey. Phase 2–3.
- **(f) Input-injection correctness/security tests.** Phase 1–2.

## 14. Security Operations (new section — completeness gap)
- Ship `SECURITY.md` **with a `security.txt`**, a coordinated-disclosure SLA (48h ack), and a CVE/advisory channel.
- **Key-compromise runbook** (pre-written) for the signing key **and** the device-enrollment CA (the highest-leverage asset) — what happens *when*, not *if*.
- **Forced-update / revoke-bad-version** (roll-forward-mandatory) mechanism in the auto-update design, distinct from rollback protection.
- **User-facing rogue-device response:** the local audit log makes a rogue paired device detectable; the product gives a clear detect→revoke flow.
- Pinned **TLS fetch channel** for the update binary (distribution channel treated as untrusted, separate from signing).

## 15. Legal-Beyond-Codecs (new — completeness gap; counsel-sign-off gates)
- **(a) Crypto export control:** downloadable E2EE → US **EAR Category 5 Part 2**; needs self-classification / CCATS posture, annual self-report, and an **OFAC-sanctioned-country download block**.
- **(b) NVIDIA Video Codec SDK** redistribution terms **and** the GeForce consumer **NVENC session-count cap** as a documented multi-session host constraint.
- **(c) GDPR/CCPA as a hosted relay/signaling operator:** you are a controller/processor for connection metadata (IPs, presence, timing) even though you can't see pixels — privacy policy, DPA, lawful basis, data-retention design.
- **(d) Freedom-to-operate** glance at remote-desktop input-injection/latency patents; trademark care when naming competitors.
- **(e) Wiretap/two-party-consent law** gating session recording + assist-others (US state two-party-consent + GDPR).

## 16. Supported-Platform & Capability-Degradation Matrix (new — see `docs/PLATFORM-MATRIX.md`)
Minimum Windows build for WGC (Win10 2004+; Win11 behavior); TPM 2.0 vs software-key fallback behavior; per-GPU HW-encoder support + the mandatory software floor; per-window capture-exclusion behavior; **and the literal product behavior on each failed capability check (refuse vs degrade-with-warning)**. Wired into Phase-0 spike notes. Multi-monitor/DPI/hotplug/dock-undock/console-session edge cases enumerated here; several are MVP-adjacent on real hardware.
