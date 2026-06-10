# ADR-0010 — Phase 0/1 is interactive-session only; no virtual display or input driver

- **Status:** Accepted (2026-06-09) — owner call.
- **Date:** 2026-06-09.
- **Related:** `docs/ROADMAP.md` (Phase 0/1), `docs/ARCHITECTURE.md` §2, ADR-0008 (session-0 helper, Phase 2).

## Context

A remote-desktop host can take two very different routes to "see and control the machine":

1. **Interactive-session capture + inject** — capture the existing logged-in desktop (Windows
   Graphics Capture / DXGI Desktop Duplication) and inject input into that interactive session
   (Win32 `SendInput`). No special driver.
2. **A virtual display + virtual HID driver** — an indirect display driver (IddCx/WDDM) that
   creates a synthetic monitor (enables truly headless hosting, custom resolutions/EDID, and a
   dedicated capture surface) plus a virtual HID for input. Powerful, but it requires the WDF/IddCx
   driver toolchain and a long Windows **driver code-signing / attestation** track (EV certificate +
   partner-dashboard attestation signing), measured in months.

## Decision

**Phase 0 and Phase 1 use interactive-session capture + `SendInput` only. No virtual display or
virtual input driver is built in Phase 0.** The Windows driver-signing track is **not** entered yet.

A virtual-display / input driver is **revisited only if real testing proves it necessary** — e.g.,
headless/no-monitor servers, multi-monitor or resolution/EDID control the OS cannot otherwise give
us, or capture-isolation requirements. If adopted, its driver-signing track must **start at the
beginning** of whatever phase takes it on — it cannot be a late add.

## Consequences

- The MVP cannot host a **truly headless** machine (no display and no virtual display); it controls
  the real interactive desktop of a logged-in session. This matches the already-documented Phase-1
  limitation: interactive-session only, with UAC / lock-screen out of scope until the Phase-2
  session-0 helper (ADR-0008).
- **No EV-cert driver-signing dependency on the MVP critical path** — a major schedule and cost
  de-risk for a solo build.
- Headless operation and resolution/EDID control are explicitly **deferred capabilities**, to be
  reconsidered against real testing evidence.
