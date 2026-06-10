# 0008. Session-0 secure-desktop helper: own subcrate + IPC trust-boundary threat model

- **Status:** Proposed (implemented Phase 2)
- **Date:** 2026-06-09

## Context
Capturing/injecting on the UAC/lock/login (secure) desktop requires a session-0 Windows Service (LocalSystem, for DXGI Duplication there) plus an active-session helper. This is the #1 'works until a prompt appears' killer and is a multi-week, signed, privileged sub-project. Critically, the elevated helper that can inject input system-wide is, if compromised, a SYSTEM-level keystroke injector — so the IPC channel between it and the user-session agent IS a trust boundary. The draft buried it inside host-windows as an implementation detail, which would draw the privilege/IPC boundary after the fact (the exact painful re-architecture this project wants to avoid).

## Decision
Give the elevated service its own crate `host/host-windows-helper`, distinct from `host/host-windows` (the user-session agent), from the moment it is built (Phase 2). The IPC surface between them gets its own mini threat model in this ADR: authenticated, least-privilege, schema-validated, no ambient injection authority beyond the consented session. Phase 1 ships WITHOUT the helper and documents the secure-desktop limitation in-product.

## Consequences
- The privilege/IPC boundary is designed before code, not retrofitted onto a monolith.
- The helper is separately signed (and, if a virtual display/input driver is pursued — Open Q #7 — separately driver-signed via Windows Hardware Dev Center, a months-long track to start in Phase 0).
- Phase 1 ships sooner with an honest limitation rather than stalling on a 3-6 week service.
