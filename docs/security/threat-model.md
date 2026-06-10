# Threat Model (long-form)

> Companion to `docs/SECURITY.md` (the invariants) and `docs/security/crypto-spec.md` (the
> constructions). This is the v0 threat model to be approved at the Phase-0 exit gate and revised
> each phase. Scope grows with the product: the MVP is **LAN-only**, so the wide-area adversaries
> (malicious relay, signaling enumeration) are *designed for now, exercised in Phase 2*.

## 1. Organizing principle

The relay is the enemy, the network is the enemy, and a stolen device is a question of *when*.
**Compromise of any single component — relay, network, account server, or one paired device —
must not compromise the host.**

## 2. Assets (priority order)

1. The **live screen/audio + input channel** (the crown jewel — real-time control of the host).
2. The **host device-identity key**.
3. The **client device-identity keys**.
4. The **paired-devices allowlist**.
5. Account credentials / session tokens (v1.0+).
6. File-transfer / clipboard contents.
7. **Audit-log integrity**.
8. The **update / signing pipeline** (highest leverage — one compromise forges trusted binaries).

## 3. Adversaries

- **Active on-path network MITM** (hostile LAN in the MVP; hostile WAN from Phase 2).
- **Malicious / compromised relay** — assumed fully hostile; must only ever see ciphertext.
- **Stolen / lost device.**
- **Brute-force / credential attacker** (pairing-code guessing; account login at v1.0).
- **Supply-chain attacker** (dependency, build, or signing-pipeline compromise).
- **Malicious peer / over-broad consent** (the "assist others" abuse surface).

## 4. Trust boundaries

- **Endpoint ↔ network:** everything on the wire is untrusted; all three channels (media,
  control, bulk) ride inside the crypto envelope. No "compatibility/unencrypted" mode.
- **Endpoint ↔ relay:** the relay is a blind pipe; E2E keys live only in the two endpoints'
  secure elements. The relay is untrusted **even when we operate it**.
- **Interactive session ↔ secure desktop (Phase 2):** the session-0 `host-windows-helper` runs at
  higher privilege and is its own trust boundary with a dedicated IPC threat model (ADR-0008).
- **`core/crypto` and `core/audit`** are crypto-grade-owned modules; the rest of the core depends
  on them, never the reverse.

## 5. Attack surface

- The pairing ceremony (pairing-time MITM) → defeated by out-of-band **SAS** over the full Noise
  handshake transcript; **no blind TOFU**.
- The Noise handshake and AEAD record layer → vetted `snow` + `ring`/`aws-lc-rs`; fuzzed from
  Phase 0; never hand-rolled (see `crypto-spec.md`).
- **NAT traversal** → **outbound only** (ICE hole-punching + IPv6 + blind relay). **No router
  port-mapping (UPnP-IGD / NAT-PMP / PCP)** — instructing the router to open an inbound port would
  re-introduce the exact exposed surface the no-inbound-port invariant exists to remove.
- The relay (Phase 2) → TURN-over-TLS:443, ephemeral HMAC creds, SSRF-hardened
  (`denied-peer-ip` for RFC1918 / link-local / cloud-metadata `169.254.169.254`), per-allocation
  quotas.
- Capture of sensitive surfaces → WGC can capture password managers / secure-input / DRM windows;
  honor per-window capture-exclusion flags and surface the risk.
- The update channel → signature + monotonic-version + OS-native code-signature + pinned-TLS
  verification before applying (distribution channel is untrusted, separate from the signing key).

## 6. Per-adversary mitigations (summary)

| Adversary | Primary mitigation |
|---|---|
| On-path MITM | Always-on E2E AEAD; SAS-verified pairing; pinned peer keys |
| Malicious relay | "Relay is blind" — ciphertext only, never a decryption point |
| Stolen device | Secure-element device keys; revocation from any other device; offline recovery code (designed Phase 2); audit log records storage class |
| Brute-force | Pairing-attempt lockout; auth-before-presence; PoW/token on floods (Phase 2); Argon2id at rest |
| Supply chain | Reproducible builds; HSM/Sigstore signing (never a key on a dev laptop); SBOM + cargo-audit from Phase 0; SHA-pinned CI |
| Malicious peer / consent | Deny-by-default capabilities; time-boxed, logged, explicit consent; non-spoofable indicator + kill switch |

## 7. Explicitly out of scope

- An OS/kernel-level-compromised host (but the product must never be the vector).
- Nation-state hardware implants.
- Rubber-hose / coercion of the legitimate owner.

## 8. Spikes feeding this model (decided where noted; the rest close before their freeze)

- **Device-key storage (ADR-0009) — DECIDED Option A:** non-FIPS X25519 wrapped at rest by the OS
  keystore (not hardware-non-exportable); storage class recorded per device. FIPS / NIST-curve deferred.
- **FIPS posture (ADR-0009) — DECIDED: none for the MVP** (keep ChaCha20-Poly1305 / BLAKE2s +
  X25519); FIPS is a later enterprise constraint.
- **Browser E2EE on WebKit (Phase 3):** Safari Encoded-Transform support is at-risk; resolved by a
  compatibility spike, not assumed absent.
- **NAT-traversal success rate (Phase 2):** governed by the NAT test matrix SLI; relay covers the
  residual.

## 9. Residual risk

- **Revocation is best-effort until Phase 2:** a fully-P2P MVP has no rendezvous to push a
  revocation to an *offline* device — stated in product and docs.
- **Software-key devices** are weaker than hardware-backed ones; allowed only with a non-bypassable
  warning, never for unattended access or first pairing, and recorded in the audit log.
