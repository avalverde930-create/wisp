# Coordinated Disclosure Policy & SLA

> How to report a vulnerability in Secure Remote Desktop, and what to expect in return.
> Machine-readable contact lives in `security.txt`; the PGP key in `pgp-key.asc`.

## Reporting

- **Email:** security@<domain> (PGP-encrypted preferred — key in `pgp-key.asc`).
- **Please do NOT** open public issues, pull requests, or social posts for security reports.
- Include: affected component/version, reproduction steps or PoC, impact, and any suggested fix.

## Our commitments (SLA)

| Stage | Target |
|---|---|
| Acknowledge receipt | within **48 hours** |
| Initial triage + severity | within **5 business days** |
| Status updates | at least every **7 days** until resolved |
| Fix or mitigation | severity-dependent; critical issues prioritized over all feature work |
| Coordinated public disclosure | by mutual agreement, default **90 days** from report (sooner if a fix ships and is adopted; later only by agreement) |

## Severity & advisories

- Severity assessed on a CVSS-style basis (confidentiality/integrity/availability of the host and
  the live control channel weighted highest — see `threat-model.md` asset priority).
- Confirmed issues receive a **CVE** (where applicable) and a published advisory via the advisory
  channel; fixed releases are **forced-update / revoke-bad-version** where the issue warrants
  (see `key-compromise-runbook.md`).

## Safe harbor

Good-faith research that respects this policy — no privacy violations, no data destruction, no
service degradation, no access beyond what is needed to demonstrate the issue, and no disclosure
before coordinated release — will not be pursued or reported by us. Testing is authorized **only**
against your own installations/devices, never against other users.

## Scope

- **In scope:** the host agent, clients, the shared core, and (Phase 2+) the signaling/relay/API
  services we operate.
- **Out of scope (see `threat-model.md`):** OS/kernel-level host compromise, nation-state hardware
  implants, coercion of the legitimate owner, and findings that require a pre-compromised endpoint.
