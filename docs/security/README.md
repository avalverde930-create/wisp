# docs/security/

- `threat-model.md` — full long-form threat model (assets, adversaries, invariants, out-of-scope).
- `crypto-spec.md` — the Noise XX->IK construction, SFrame (Phase 3), key hierarchy, FIPS variant.
- `security.txt` — published disclosure contact + policy (also served at /.well-known/security.txt).
- `pgp-key.asc` — the security@ PGP key.
- `key-compromise-runbook.md` — pre-written response for signing-key AND device-enrollment-CA compromise (the highest-leverage asset): detection, revoke-bad-version roll-forward, re-key, user notification.
- `disclosure-sla.md` — 48h acknowledgement, coordinated-disclosure timeline, CVE/advisory channel.
- `pen-tests/` — third-party pen-test + crypto-audit reports (Phase 2 review; Phase 4 full).
