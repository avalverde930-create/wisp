# Supported Platforms & Capability Degradation

> Wired into Phase-0 spike notes. The product's behavior on each FAILED capability check is explicit: refuse vs degrade-with-warning.

## Host capability floor (Windows 11 Pro primary)
| Capability | Minimum / detection | If absent |
|---|---|---|
| WGC capture | Win10 2004+; Win11 build-dependent | fall back to DXGI Desktop Duplication; if that fails, refuse with a clear message |
| Secure-desktop (UAC/lock) capture+inject | session-0 service + LocalSystem (Phase 2) | Phase 1: documented limitation banner; control silently halts on elevated focus |
| HW H.264 encode (NVENC/QSV/AMF) | probe per-GPU | software x264 floor (mandatory) — degrade-with-warning |
| GeForce NVENC session-count cap | ~3-5 concurrent (consumer) | cap concurrent sessions; surface the limit |
| TPM 2.0 secure element | CNG Platform Crypto Provider — **NIST curves only; X25519-static storage is a Phase-0 spike (ADR-0009)** | software-protected key with a NON-bypassable warning; refuse unattended access and first-pairing on software keys |
| Per-window capture exclusion | SetWindowDisplayAffinity / WDA_EXCLUDEFROMCAPTURE | honor exclusion; surface that sensitive windows may be captured |

## Display-topology edge cases (enumerate; several MVP-adjacent)
Monitor hotplug, resolution change mid-session, RDP/console-session transitions, laptop dock/undock, HiDPI scaling.

## Client platforms
| Platform | Role | Phase | Note |
|---|---|---|---|
| Windows/macOS/Linux desktop | client + host | host Win MVP; mac/Linux host Phase 3 | the day-one dogfood client is desktop |
| iOS | **client only** | Phase 3 | no 3rd-party background screen-capture entitlement -> iOS HOST infeasible; ReplayKit/background limits; App Store remote-control review risk |
| Android | client | Phase 3 | Play AccessibilityService/remote-access policy must be satisfied |
| Web | client | Phase 3 | **Chromium-family confirmed.** Safari/WebKit Encoded-Transform support is a **Phase-3 compatibility spike (Open Q #13), not a permanent exclusion** |

## Distribution gatekeepers
- iOS/macOS App Store actively scrutinizes remote-control apps (e.g., Screens rejection Aug 2025). macOS self-host build: notarization + Gatekeeper.
- Microsoft Store vs direct signed download: direct download likely default (SmartScreen-reputation caveat, see TECH-STACK / SECURITY).
- Fallback: direct download / TestFlight / enterprise if a store rejects. Desktop-first dogfood sidesteps store risk for the MVP.
