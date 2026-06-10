# Running the Phase-0a LAN spike

The Phase-0a vertical spike proves the core loop end to end: **GDI primary-monitor
capture → LZ4 → QUIC (quinn) → decode → softbuffer render**, with **mouse/keyboard
input** sent back and injected via Win32 `SendInput`. It is the thinnest thing that
lets you *see and control* one Windows PC from another on the same LAN.

> ⚠️ **SPIKE — NOT THE PRODUCT'S SECURITY MODEL.** Two simplifications remain, with a
> guardrail bolted on:
> 1. The QUIC transport **skips TLS certificate verification** (throwaway self-signed
>    cert). The spike adds a **pre-shared token** (below) so a LAN bind can't silently
>    accept unauthenticated control — but the token crosses the cert-unverified channel
>    in cleartext, so it stops *casual* access, **not an active MITM**. Real
>    authentication + confidentiality is the Phase-1 **Noise XX/IK + SAS pairing**
>    (ADR-0003), not built yet.
> 2. The host **opens an inbound UDP port** to listen directly. This *contradicts the
>    product invariant* (ADR-0005: the host never opens an inbound port; it dials out to
>    a rendezvous broker). Phase 2 replaces direct listening with outbound signaling +
>    NAT hole-punching. The spike listens directly only because it is LAN-only.
>
> **Do not port-forward this to the internet.** For real remote access today, use
> Tailscale + Windows RDP.

## Prerequisites

- Windows 11 (host = the PC you want to control).
- Rust toolchain + MSVC build tools (already installed on this machine).
- Two machines on the same LAN — **or** just use the loopback default on one machine to smoke-test.

## Build

```powershell
# from the repo root: 03 TECHNOLOGY\Secure Remote Desktop
cargo build --release
```

Use **`--release`**. The debug build is ~7–10× slower (LZ4 + pixel loops are pathological
unoptimized): ~2 fps debug vs ~19 fps release at 1080p in local testing.

## Run — the host (the PC being controlled)

```powershell
# Same-machine test: binds 127.0.0.1 only, no token needed (loopback is the trust boundary).
target\release\host-windows.exe

# LAN: set a shared secret FIRST, then bind to all interfaces. The host REFUSES a
# non-loopback bind without SRD_SPIKE_TOKEN, so LAN control is never unauthenticated.
$env:SRD_SPIKE_TOKEN = 'choose-a-strong-secret'
target\release\host-windows.exe 0.0.0.0:9000
```

It prints the primary-monitor size, the auth mode, and waits for a client. **Interactive
session only** (ADR-0010): it cannot capture or control the UAC prompt, the secure desktop,
or the lock screen — any elevated window silently halts control (that's the Phase-2
session-0 helper's job).

**Windows Firewall (LAN only):** allow inbound UDP 9000 on the host's *private* profile:

```powershell
# elevated PowerShell on the host
netsh advfirewall firewall add rule name="SRD spike UDP 9000" dir=in action=allow protocol=UDP localport=9000 profile=private
```

## Run — the client (the PC you control *from*)

```powershell
# LAN: set the SAME token the host used, then connect.
$env:SRD_SPIKE_TOKEN = 'choose-a-strong-secret'
target\release\client.exe <HOST-LAN-IP>:9000      # example: ...client.exe 192.168.1.50:9000

# Same-machine test (no token): just
target\release\client.exe 127.0.0.1:9000
```

A window opens showing the host's desktop (nearest-neighbor scaled to the window). Move the
mouse, click, scroll, and type to control the host. The **window title shows the live latency
numbers**: received FPS + QUIC path RTT. A wrong/missing token gets you **zero frames** —
the host withholds the screen and ignores input until the token validates.

Headless benchmark (no window — just the numbers):

```powershell
target\release\client.exe <HOST-IP>:9000 --bench
```

## What works in 0a / what's deferred

| Works now (0a) | Deferred |
|---|---|
| GDI full-frame capture (primary monitor) | WGC capture + dirty-rects (**0b**) |
| LZ4 frame compression | Hardware H.264 NVENC/QSV/AMF, x264 floor (**0b**) |
| QUIC transport (quinn) + pre-shared-token gate | Noise XX/IK + SAS pairing — real E2E security (**Phase 1**) |
| softbuffer CPU render (scaled) | wgpu GPU render path (**0b**) |
| Mouse move/click/scroll + keyboard via `SendInput` | Clipboard, file transfer, audio, multi-monitor (**Phase 1+**) |
| LAN direct connect | Outbound rendezvous + NAT traversal + relay (**Phase 2**) |
| Latency numbers (fps + RTT) | UAC / lock-screen control via session-0 helper (**Phase 2**) |

## Measured (local loopback, release)

~13–20 fps @ 1920×1080, RTT < 1 ms, ~0.5–0.6 MiB/frame on the wire, decode integrity verified,
token gate verified (tokenless client → 0 frames). The fps ceiling is the GDI full-frame +
software-LZ4 path — exactly what **Phase-0b** (WGC + hardware H.264 + GPU zero-copy + dirty-rects)
exists to fix, targeting 30–60 fps at < 50 ms glass-to-glass.
