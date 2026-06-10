# Key-Compromise Runbook

> Pre-written response for compromise (or suspected compromise) of the two highest-leverage
> assets: the **code-signing key** and the **device-enrollment CA**. Written before launch so the
> response is rehearsed, not improvised. Owner is the incident lead until a team exists.

## Scope: the two crown-jewel keys

1. **Code-signing key** — forging this lets an attacker ship a trusted malicious update.
2. **Device-enrollment CA** (Phase 2+) — forging this lets an attacker enroll rogue devices into a
   trust set.

Both must live in an HSM / Sigstore-backed flow with Rekor transparency — **never** on a dev
laptop. If either ever touched unmanaged storage, treat as suspected-compromise.

## A. Detection / triggers

- Rekor/transparency-log entry the owner did not initiate.
- Unexpected signed artifact, version, or enrollment.
- HSM access anomaly, lost/stolen signing hardware, or a leaked secret detected by gitleaks/secret
  scanning.
- Credible third-party report via the disclosure channel (`disclosure-sla.md`).

## B. Containment (first hour)

1. **Revoke** the affected key / certificate at the issuer and in the app's pin set.
2. **Freeze releases:** disable the CI signing job (it is SHA-pinned and key-gated); no new signed
   artifacts until re-key.
3. **Preserve evidence:** snapshot CI logs, HSM audit logs, and the transparency log.
4. **Assess blast radius:** which versions/devices could have been signed/enrolled in the exposure
   window.

## C. Eradication & recovery

1. **Re-key:** generate a new signing key / CA in the HSM; update the client pin set.
2. **Roll-forward, mandatory:** publish a new signed version and use **revoke-bad-version /
   forced-update** (distinct from rollback protection) so clients refuse the compromised
   version(s). Clients verify signature **+ monotonic version + OS-native code signature + pinned-
   TLS fetch** before applying.
3. **Device-CA case:** push a signed, monotonically-versioned trust-list update revoking any
   devices enrollable during the window; endpoints reject older list versions.

## D. Notification

- Publish a security advisory (CVE if applicable) via the advisory channel in `disclosure-sla.md`.
- Notify affected users in-product and out-of-band; state the forced-update requirement plainly.
- Update `security.txt` if contacts/keys changed.

## E. Post-incident

- Root-cause writeup; tighten the control that failed (HSM policy, CI scope, secret handling).
- Rehearse this runbook against the finding; record lessons learned.

## Invariants this runbook protects

- The update channel is untrusted and **separate** from the signing pipeline.
- Roll-forward-mandatory is distinct from rollback protection — both must hold.
- No signing key on a developer machine, ever.
