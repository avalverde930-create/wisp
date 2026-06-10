# host/

The host agent — the machine being viewed/controlled. Depends on `core`; never on a client/app.

## Trust boundary & privilege model
- The host makes only OUTBOUND connections to signaling (Phase 2+); it opens NO inbound public port. The MVP is LAN-only (direct QUIC), but the outbound-only mental model is baked in now.
- It accepts sessions only from already-paired devices (SAS-verified), enforces the deny-by-default capability model, and shows a non-spoofable in-session indicator + kill switch.

## Secure-desktop reality (the #1 'works until a prompt appears' killer)
Capturing/injecting on the UAC/lock/login screen requires a session-0 Windows Service (LocalSystem, for DXGI Duplication on the secure desktop) PLUS an active-session helper. This is a Phase-2 deliverable (a signed, privileged, session-crossing service ~3-6 solo weeks). Phase 1 runs purely in the interactive session and DOCUMENTS the gap in-product. The helper is its OWN subcrate (`host-windows-helper`, Phase 2) because the IPC channel to the user-session agent IS a trust boundary (a compromised helper is a SYSTEM-level keystroke injector) — ADR-0008 draws that boundary before the code.

## Crates
- `host-core` — OS-agnostic agent: session orchestration, policy/consent enforcement, drives core::audit.
- `host-windows` — WGC capture (DXGI fallback), HW encode (Media Foundation/NVENC), SendInput injection, in interactive session. **Primary near-term target / MVP.**
- `host-windows-helper` — **DEFERRED to Phase 2**: the elevated session-0 service for secure-desktop capture/inject, with its own ADR-0008 IPC threat model.
- `host-macos` / `host-linux` — Phase 3 (ScreenCaptureKit/CGEvent; PipeWire/portal/libei).

## Dependency rules
Implements the FrameSource/VideoEncoder/InputSink traits (extracted in Phase 1's back half) from core. Platform syscalls live here, behind those traits, so core stays OS-agnostic.
