# Running the Wisp LAN spike

The spike proves the core loop end to end: **GDI primary-monitor capture → LZ4 → QUIC
(quinn) → decode → softbuffer render**, with **mouse/keyboard input** sent back and injected
via Win32 `SendInput`. It is the thinnest thing that lets you *see and control* one Windows
PC from another on the same LAN.

The transport is now **real end-to-end encryption**: a Noise handshake authenticates the
two devices by their long-term keys, derives a session AEAD, and prints a 6-digit SAS you
compare out-of-band on first contact. The media path has grown Phase-0b options behind
environment variables: **WGC** native-resolution capture (`WISP_CAPTURE=wgc`) and **H.264**
video (`WISP_CODEC=h264`) alongside the default GDI capture + LZ4 interframe codec. The render
path is still softbuffer (wgpu is the remaining 0b item). See "Capture & codec options" below.

> ⚠️ **Still a spike in two respects** (neither is the security boundary anymore):
> 1. The QUIC TLS layer uses a **throwaway self-signed cert with verification skipped**.
>    This is no longer a security gap: the **Noise layer on top** authenticates the peers by
>    their pinned static keys independently of TLS, so a forged cert buys an attacker nothing.
>    (Phase 2 still tidies this up with a proper transport cert.)
> 2. The host **opens an inbound UDP port** to listen directly. This *contradicts the product
>    invariant* (ADR-0005: the host never opens an inbound port; it dials out to a rendezvous
>    broker). Phase 2 replaces direct listening with outbound signaling + NAT hole-punching.
>    The spike listens directly only because it is LAN-only.
>
> **Do not port-forward this to the internet.** For real remote access today, use
> Tailscale + Windows RDP.

## Security model (what protects you now)

- **Noise E2E (ADR-0003).** First contact runs `Noise_XX_25519_ChaChaPoly_BLAKE2s`: both
  devices exchange and authenticate their long-term static keys, then everything after the
  handshake — the access token, input events, and screen frames — is ChaChaPoly-encrypted.
- **SAS pairing.** On first contact each side prints a 6-digit **Short Authentication String**
  derived from the handshake transcript. **Compare them out-of-band** (read it aloud / over a
  trusted channel). Matching SAS ⇒ no man-in-the-middle. This is the one manual step.
- **Key pinning (ADR-0003).** The host remembers each paired client's static key
  (`trusted-clients.txt`); the client remembers each host's static key (`known-hosts.txt`,
  keyed by address). A non-loopback client whose key is not pinned is **rejected** unless the
  host is in pair mode. A *changed* key on a known address is surfaced as a warning, never
  silently trusted.
- **0-RTT reconnect (Noise IK).** After the first XX, the client reconnects with `Noise_IK`
  using the cached host key — a 2-message handshake instead of 3, with no SAS step.
- **Persistent device identity (ADR-0009 Option A).** Each side's static key is generated
  once and stored at rest, wrapped by **Windows DPAPI** (per-user; another user/machine
  cannot unwrap it). Identities are stable across runs, which is what makes pinning meaningful.
- **Token guardrail.** A non-loopback bind additionally requires a shared `WISP_TOKEN` on
  both ends; it is sent **inside** the Noise channel (encrypted), as a second factor on top
  of key pinning. Loopback needs no token (the local machine is the trust boundary).

Key material lives under `%APPDATA%\wisp\`: `host-device.key` / `client-device.key` (DPAPI
blobs), `trusted-clients.txt` (host's pinned clients), `known-hosts.txt` (client's pinned
hosts).

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

## Run — same machine (loopback smoke test)

```powershell
# Host: binds 127.0.0.1 only, no token needed (loopback is the trust boundary).
target\release\host-windows.exe

# Client (another terminal): a window opens showing this PC's own desktop.
target\release\client.exe 127.0.0.1:9000
```

The first loopback connect runs XX (prints a SAS — no need to compare it on loopback); later
connects reconnect 0-RTT via IK automatically.

## Run — across the LAN (two machines)

### 1. Host (the PC being controlled)

```powershell
# Set a shared secret FIRST. The host REFUSES a non-loopback bind without WISP_TOKEN.
$env:WISP_TOKEN = 'choose-a-strong-secret'

# The FIRST time a given client connects, run in pair mode so the host pins its key:
$env:WISP_PAIR = '1'
target\release\host-windows.exe 0.0.0.0:9000
```

It prints its **device fingerprint**, the primary-monitor size, the auth mode, and waits.
After the client has paired once, drop `WISP_PAIR` — the client is now pinned and pair mode
is no longer needed (an unknown new device would be rejected).

**Interactive session only** (ADR-0010): it cannot capture or control the UAC prompt, the
secure desktop, or the lock screen — any elevated window silently halts control (that's the
Phase-2 session-0 helper's job).

**Windows Firewall (LAN only):** allow inbound UDP 9000 on the host's *private* profile:

```powershell
# elevated PowerShell on the host
netsh advfirewall firewall add rule name="Wisp UDP 9000" dir=in action=allow protocol=UDP localport=9000 profile=private
```

#### Capture & codec options (host-side env vars)

Both default to the safe, lowest-dependency path; set them before launching the host.

| Variable | Default | Set to `…` | Effect |
|---|---|---|---|
| `WISP_CAPTURE` | GDI (DPI-scaled logical res) | `wgc` | Windows.Graphics.Capture at the monitor's **native** resolution (ADR-0011 4b). Falls back to GDI if WGC init fails. |
| `WISP_CODEC` | LZ4 interframe — GOP keyframe + XOR delta (4a) | `h264` | Media Foundation **H.264**, software MFT, low-latency (4c) — far smaller on the wire. Falls back to LZ4 if H.264 is unavailable. |

```powershell
# example: native-resolution WGC capture + H.264 on the wire
$env:WISP_CAPTURE = 'wgc'; $env:WISP_CODEC = 'h264'
target\release\host-windows.exe 0.0.0.0:9000
```

Host diagnostics (run and exit): `--probe-wgc` (WGC init + monitor size), `--probe-h264` (list
H.264 encoder MFTs), `--selftest-h264` (encode→decode round-trip), `--selftest-capture` (real
capture → H.264). The client decodes whichever codec each frame's header declares — no client
flag needed.

### 2. Client (the PC you control *from*)

```powershell
# Set the SAME token the host used, then connect.
$env:WISP_TOKEN = 'choose-a-strong-secret'
target\release\client.exe <HOST-LAN-IP>:9000      # example: ...client.exe 192.168.1.50:9000
```

A window opens showing the host's desktop (nearest-neighbor scaled to the window). Move the
mouse, click, scroll, and type to control the host. The **window title shows the live latency
numbers**: received FPS + QUIC path RTT.

### 3. Verify the SAS (first connect only)

On the **first** connect both sides print `pairing SAS: NNNNNN`. **Compare the two numbers.**
They must match — if they do, the channel is free of a man-in-the-middle and the host pins the
client. If they differ, stop: someone is between you. On every later connect the client uses an
IK 0-RTT reconnect (no SAS step) and the host logs `IK 0-RTT reconnect`.

A wrong/missing token, or an unpinned device when the host is **not** in pair mode, gets you
**zero frames** — the host withholds the screen and ignores input.

Headless benchmark (no window — just the numbers):

```powershell
target\release\client.exe <HOST-IP>:9000 --bench
```

## What works now / what's deferred

| Works now | Deferred |
|---|---|
| **Noise XX/IK + SAS pairing — real E2E auth + confidentiality** (Phase 1) | Proper transport cert instead of cert-skip (**Phase 2**) |
| **Key pinning** (host pins clients, client pins hosts) | — |
| **IK 0-RTT reconnect** (cached host key) | — |
| **Persistent device identity, DPAPI-wrapped at rest** (ADR-0009) | OS-keystore on macOS/Android; recovery-code slot (**Phase 1+**) |
| GDI **and** WGC capture (`WISP_CAPTURE`, native-res) — 4b | Dirty-rects / damage regions (**0b**) |
| LZ4 interframe (GOP + XOR delta, 4a) **and** H.264 (`WISP_CODEC`, 4c) | Hardware QSV/NVENC/AMF **async** encode + GPU colour convert (**0b**) |
| softbuffer CPU render (scaled) | wgpu GPU render path + zero-copy (**0b**) |
| Mouse move/click/scroll + keyboard via `SendInput` | Clipboard, file transfer, audio, multi-monitor (**Phase 1+**) |
| LAN direct connect | Outbound rendezvous + NAT traversal + relay (**Phase 2**) |
| Latency numbers (fps + RTT) | UAC / lock-screen control via session-0 helper (**Phase 2**) |

## Measured (local loopback, release)

Per-frame wire size by codec (mostly-static 1536×864 desktop, decode integrity verified every
frame):

| Codec | Bytes/frame | |
|---|---|---|
| raw LZ4 full-frame (Phase-0a) | ~165 KiB | baseline |
| LZ4 interframe (4a, **default**) | ~22 KiB | GOP keyframe + XOR delta |
| H.264 software (4c, `WISP_CODEC=h264`) | ~5 KiB | low-latency MFT |

Frame rate (both processes sharing one CPU on loopback): ~20 fps GDI + LZ4 interframe, ~11 fps
GDI + software H.264, ~6 fps WGC native-4K + LZ4. Noise E2E with SAS match; IK 0-RTT reconnect
on later connects. The ceiling now is **software encode + CPU colour conversion**, not bandwidth
— exactly what the remaining **0b** work (hardware QSV async encode, wgpu GPU render + zero-copy
colour) targets to reach 30–60 fps at < 50 ms glass-to-glass.
