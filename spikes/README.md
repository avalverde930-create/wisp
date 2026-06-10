# spikes/

**Deliberately throwaway.** Phase-0 de-risking. The four risks are NOT independent — latency/capture/transport only mean something measured END-TO-END — so this is ONE sequenced vertical spike that becomes Phase 1, not four parallel one-page notes.

- `vertical/` — WGC capture -> HW H.264 encode -> quinn loopback -> decode -> present -> SendInput, measuring glass-to-glass latency (<50 ms LAN target). Becomes the Phase-1 thin slice.
- `nat/` — the ONE genuinely separate spike (doesn't compose into a LAN slice): ICE/UDP hole-punching success vs home/CGNAT/symmetric NAT + a trivial relay. Run in Phase 2 prep.
- `transport-soak/` — confirm quinn for native; NOTE (do not build) the webrtc-rs-vs-str0m bake-off for Phase 3 (memory-leak soak, in-process attack surface, fuzzability).
- `winenv/` — FIRST spike: stand up the Windows graphics dev env (Media Foundation / NVENC SDK / D3D11 interop / cross-process GPU texture sharing).

Each spike ends in a go/no-go note recorded under docs/ or as an ADR. Delete the code after the findings land.
