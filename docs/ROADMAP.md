# Roadmap

**Build the hard, scary part first — as a RUNNING ARTIFACT, not an abstraction.** The biggest solo-build risk is months with nothing that runs. Phase 0 and Phase 1 are ONE sequenced vertical slice.

## Solo-developer calendar reality (blunt)
Phase 0 spike ~2-4 weeks. Phase 1 thin LAN slice ~4-6 weeks. Phase 2 secure-internet ~2-4 months. Do not start Phase 3 until Phase 2 has been dogfooded for weeks. **Prerequisite: two physical machines (or a 2nd GPU/VM) on one LAN.** First spike of all: stand up the Windows graphics dev env (Media Foundation / NVENC SDK / D3D11 interop / cross-process GPU texture sharing).

## Phase 0 — One vertical de-risking spike (throwaway) + repo skeleton
ONE sequenced vertical spike (it becomes Phase 1): WGC capture -> HW H.264 encode -> quinn loopback -> decode -> present -> SendInput, measuring glass-to-glass latency (<50 ms LAN). NAT traversal stays a SEPARATE Phase-2 spike (only risk that doesn't compose into a LAN slice). Transport soak: confirm quinn for native; NOTE (don't build) the webrtc-rs-vs-str0m bake-off for Phase 3. Deliver: skeleton (core/, host/, client/, spikes/, docs/adr/), threat model v0, CI security gates (cargo-audit, SBOM, secret scan), Azure Trusted Signing wired, ADR-0001/0002/0003. **Plus the device-key-storage spike (ADR-0009) — DECIDED Option A:** implement X25519 wrapped at rest by the OS keystore (no FIPS), recording the storage class to the audit layer. (FIPS / NIST-curve is a later enterprise constraint.)
**FIPS + device-key-storage gate — DECIDED 2026-06-09:** **No FIPS for the MVP**; device keys use **Option A** (X25519 + OS-keystore-wrapped, ADR-0009). FIPS / NIST-curve is a later enterprise constraint. **Also decided: no virtual display / input driver in Phase 0** (interactive-session control only, ADR-0010) — the months-long Windows driver-signing track is deferred until real testing proves it necessary.
**Exit:** live frame + mouse on loopback; <50 ms measured; protocol/trait TARGET reviewed (not yet coded); threat model v0 approved; FIPS **and device-key storage decided (No FIPS / Option A — done)**; CI green.

## Phase 1 — LAN-only MVP (one Windows host <-> one desktop client)
SINGLE host binary + SINGLE client binary, ONE crate, same-LAN, mDNS/manual-IP, WGC primary-monitor capture -> HW H.264 (x264 fallback) -> plain Noise(XX)-encrypted quinn -> winit+wgpu render + SendInput. Hand-written wire structs (core/wire). Owner approval to start; SAS pairing code compared on both screens every session; non-spoofable indicator + one-click/hotkey kill switch; clear 'cannot control elevated prompt / UAC / lock screen — known limitation' banner. NO proto/buf, NO Tauri, NO services, NO traits-as-code yet.
**Definition of done (hold in your head):** On two LAN machines, run host.exe on PC-A + client.exe on PC-B, enter PC-A's IP, compare a 6-digit pairing code and accept, then see PC-A's primary monitor at ~1080p30 and control mouse/keyboard, all bytes Noise-encrypted, a banner on PC-A showing the live session + a kill key. UAC/lock-screen explicitly not supported yet. Everything not in this sentence is deferred.
**Exit:** DoD met; reconnect after Wi-Fi blip (QUIC migration); kbd+mouse+scroll; pairing every session; no unauthenticated path can start a session; signed installer (SmartScreen caveat documented).

## Phase 2 — Secure remote over the internet (security-defining)
Now versioned proto/ + buf (Rust/prost) replaces core/wire; traits EXTRACTED from working Phase-1 code. Zero-knowledge signaling (rate-limited, auth-before-presence, blinded rendezvous IDs); ICE/STUN/**outbound** hole-punching + three-tier fallback (direct -> IPv6 -> relay), **no router port-mapping (UPnP/NAT-PMP/PCP)**; coturn/eturnal relay (ADR-0007, ciphertext only, ephemeral HMAC creds, TURN-TLS:443, SSRF-hardened); full E2E + SAS pairing hardened (device-bound keys, forward secrecy XX->IK, mutual auth, revocation, recovery-code design implemented); host hardening (default-deny inbound, paired-only, core/audit hash-chained log); the session-0 host-windows-helper (secure-desktop capture/inject) with ADR-0008 IPC-boundary threat model; connection-quality adaptation; OTel + first Tier-0 Docker Compose deploy. Test deliverables: NAT-traversal matrix harness (P2P-success-rate SLI), relay abuse tests (SSRF/open-proxy/exhaustion), wire-parser + handshake cargo-fuzz in CI.
**Exit:** direct-connect across >=3 network types, relay covers the rest; all traffic E2E, relay/signaling provably can't decrypt; no inbound port on host; revocation + recovery work; secure-desktop helper passes its IPC threat-model review; reconnects survive IP changes; external crypto+pairing review done; fuzz green.

## Phase 3 — Multi-platform clients + host portability groundwork
UniFFI (iOS SwiftUI / Android Compose) + wasm-bindgen+webrtc-rs (web). Now the TS toolchain (pnpm/Turborepo/ts-proto/packages/*), Tauri desktop UI, check-dep-direction.sh, cargo-hakari are introduced (they earn their keep at the 2nd language / 2nd app). Browser E2EE via SFrame/Insertable-Streams (Chromium-family). Mobile push-to-wake (APNs/FCM). Add the webrtc-rs MediaTransport (after the soak bake-off vs str0m). Host spikes: ScreenCaptureKit+accessibility (mac), PipeWire/portal+libei (Linux). File transfer + clipboard (consent-gated, inside the crypto envelope), multi-monitor, HiDPI. Cross-encoder/decoder interop matrix (NVENC<->VideoToolbox<->WebCodecs).
**Exit:** >=3 new client platforms with view+control parity; **web works on Chromium-family browsers; Safari/iOS-web is gated on a WebKit Encoded-Transform compatibility spike (Open Q #13) — supported iff the spike confirms a working Encoded Transform + SFrame path, otherwise served by the native iOS client**; one non-Windows host at Phase-1 capability behind a flag; no client weakens crypto/pairing.

## Phase 4 — Polish, scale, optional SaaS
AV1 where available, GPU-decode clients, 4K/120; autoscaling, SLOs, tracing, runbooks, automated cert rotation; optional SaaS (orgs, RBAC, device inventory, exportable audit logs, signed offline self-host licenses, OPAQUE account login, billing); consent-based assist-others (time-boxed, logged, architecturally distinct); pen-test, bug bounty, SLSA L3 attestation, SOC 2 Type 2 (when procurement asks); cross-device audit anchoring; FIDO2 attestation (wisp/v2).
**Exit:** SLOs met under load; relay/signaling scale horizontally; signed auto-updates; pen test passed, no open criticals; self-host path documented and working; assist-others behind explicit opt-in.

## Deferred-slot ledger (reserved, NOT deleted)
| Slot | Lands in |
|---|---|
| proto/ + buf (Rust) | Phase 2 |
| proto ts-proto codegen + TS toolchain (pnpm/Turborepo/packages/*) | Phase 3 |
| split core/* into 7 crates; cargo-hakari/workspace-hack | Phase 2-3 (when forced) |
| check-dep-direction.sh + CODEOWNERS + ESLint/Turbo tags | Phase 3 / first hire |
| services/{signaling,relay-ctl} | Phase 2 |
| services/identity (OPAQUE, billing, licenses) + api-gateway | v1.0 / Tier 2 |
| host/host-windows-helper (session-0) | Phase 2 |
| bindings/{uniffi-mobile,wasm}; clients-native/{ios,android}; apps/{desktop(Tauri),web,web-site} | Phase 3 |
| infra/terraform | Tier 1 |
| infra/{k8s,ansible} | Tier 2 |
| tests/{e2e,load} | Phase 3 / Tier 1 |
| SFrame; cross-device audit anchoring; FIDO2 attestation; capability tokens | Phase 3-4 / wisp/v2 |
