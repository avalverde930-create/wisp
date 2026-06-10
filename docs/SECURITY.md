# Security

## Disclosure
Report vulnerabilities to security@<domain> (PGP key + security.txt in `docs/security/`). We aim to acknowledge within 48h. Please do not open public issues for security reports.

## Organizing principle
The relay is the enemy, the network is the enemy, and a stolen device is a question of *when*. Compromise of any single component must not compromise the host.

## Solo-builder discipline
Over-built security is dangerous: a solo builder who spreads review across ten mechanisms audits none to depth. **Fewer, deeper > more, shallower.** The MVP ships exactly THREE mechanisms done correctly: (1) the Noise channel, (2) SAS pairing + secure-element device keys, (3) outbound-only host + local audit log. OPAQUE, SFrame, attestation, mTLS, cross-device anchoring, capability tokens are each deferred to the phase that needs them.

## Invariants (non-negotiable)
1. **The host opens no inbound public port, ever.** Outbound-only (Phase 2+). No UPnP-IGD, no NAT-PMP/PCP, no router port-mapping of any kind, no auto port-forward, no 'expose to internet' toggle. NAT traversal is *outbound only* — ICE hole-punching + IPv6 + blind relay — never asking the router to open a port.
2. **End-to-end encryption is always on and non-optional.** No 'compatibility/unencrypted' mode — that switch becomes the downgrade attack.
3. **The relay is blind.** Forwards only ciphertext; never a decryption point. E2E keys live only in the two endpoints' secure elements.
4. **Pairing requires out-of-band SAS verification.** Never blind TOFU. A changed device key triggers a loud warning and forces re-pair.
5. **Deny-by-default capabilities.** VIEW / CONTROL / CLIPBOARD / FILE_TRANSFER / AUDIO / MULTI_MONITOR are separate grants; default view-only; unattended access off by default and only on hardware-backed keys.
6. **Non-spoofable in-session indicator + one-click kill switch** (OS-controlled) + global hotkey.
7. **Audit log is append-only, hash-chained, and strictly local** — never shipped to relay or cloud. Lives in `core/audit` (crypto-grade ownership). Records key-storage class per device.
8. **All three wire channels (media/control/bulk) are inside the crypto envelope.** No exemptions for file transfer.

## Crypto choices
- **Native E2EE (MVP/v1): Noise_XX_25519_ChaChaPoly_BLAKE2s for first contact (full mutual auth + SAS), cache the peer static, Noise_IK for 0-RTT reconnect, CLEAN re-pair on IK decrypt failure.** XXfallback / Noise Pipes is intentionally NOT used: the `snow` crate does not implement the fallback modifier and hand-rolling a handshake fallback is forbidden. See `adr/0003-noise-pattern.md`.
- **Browser E2EE (Phase 3 / v1.0):** WebRTC DTLS 1.3 + mandatory SFrame via Encoded-Transform/Insertable-Streams, keyed from a Noise/ECDH agreement (not DTLS). No Insertable-Streams support => refuse the session. **WebKit/Safari Encoded-Transform support is treated as AT-RISK, not excluded:** it is resolved by a **Phase-3 browser-compatibility spike (Open Q #13), not a permanent product decision** (WebRTC Encoded Transforms are now broadly specified; WebKit shipped Safari-18 encoded-transform fixes — hence re-test at Phase 3). Chromium-family is the confirmed target; Safari/iOS-web ships iff the spike confirms a working Encoded Transform + SFrame path, otherwise iOS is served by the native UniFFI client.
- **Account login (v1.0 only): OPAQUE (RFC 9807).** The MVP has NO account and NO password — device pairing is the only auth. Local at-rest: Argon2id (OWASP-2025 params).
- **Device identity:** a static keypair held in / wrapped by the secure element (Windows TPM 2.0 via CNG, Apple Secure Enclave, Android StrongBox). **Open composition risk (Phase-0 spike, ADR-0009):** Windows CNG/TPM and its FIPS Platform Crypto Provider expose **NIST ECDH curves (P-256/384), not Curve25519**, while Noise/`snow` uses **X25519** — so a raw X25519 static may not be generatable as a non-exportable key *inside* the TPM. **DECIDED (2026-06-09, ADR-0009): Option (a)** — **non-FIPS X25519 with the private key wrapped at rest by the OS keystore** (Windows DPAPI / CNG software KSP) rather than generated-non-exportable-in-hardware. Option (b), a **FIPS / NIST-curve handshake**, is a **later enterprise constraint**, not built for the MVP. So "non-exportable in the TPM" is explicitly **not** an MVP guarantee — the MVP ships software-wrapped X25519 keys with the storage class recorded. Software-key degradation is allowed only with a non-bypassable warning (and never for unattended access or first pairing); the audit log records storage class. Per-session forward secrecy; signed monotonically-versioned revocation (best-effort until the Phase-2 signaling layer can push to offline devices).
- **FIPS gate — DECIDED (2026-06-09): No FIPS for the MVP.** Keep ChaCha20-Poly1305 / BLAKE2s + X25519 (ADR-0009 Option A). FIPS (AES-GCM/SHA-2 + NIST curves) is a **later enterprise constraint**, revisited only if a customer compliance bar requires it.

## Recovery (designed before the key hierarchy freezes)
Losing the only paired device must not permanently brick host access. An enrollment-time offline recovery code is designed in Phase 2; its slot in `core/identity` + the wire schema is reserved now so it is never a bolt-on backdoor.

## Capture of sensitive surfaces
WGC captures password managers / secure-input fields / DRM windows. The product surfaces this and honors per-window capture-exclusion flags where the OS provides them.

## Do-not-hand-roll
Compose vetted libraries (snow, ring/aws-lc-rs, libsodium/dryoc, rustls/quinn, audited OPAQUE [v1.0], argon2, OS keystores). All crypto in `core/crypto` and fuzzed from Phase 0. Budget one external crypto audit before any public 'secure' claim.

## Security operations
- `security.txt` + coordinated-disclosure SLA + CVE/advisory channel.
- Pre-written key-compromise runbook for the signing key AND the device-enrollment CA (the highest-leverage asset).
- Forced-update / revoke-bad-version (roll-forward-mandatory), distinct from rollback protection.
- Update fetch over pinned TLS (distribution channel untrusted, separate from the signing pipeline).

## Supply chain
Reproducible/hermetic builds targeting SLSA Build L3 (attestation deferred to the pre-'secure'-claim audit; cargo-audit + SBOM from Phase 0). Sigstore/cosign + Rekor (or HSM keys) — never a signing key on a dev laptop. GitHub Actions pinned by commit SHA. Client verifies signature + monotonic version + OS-native code signature + pinned-TLS fetch before applying any update.

Full long-form threat model: `docs/security/`.
