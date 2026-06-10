# tools/

Developer tooling and CI scripts. Not shipped to users.

## Scripts (MVP)
- `scripts/bootstrap.sh` — one-shot env setup (mirrors `just bootstrap`).
- `ci/` — shared CI helper scripts (security gates: cargo-audit, SBOM via Syft, Trivy/Grype, gitleaks/trufflehog; the FAIL-on-plaintext-key and FAIL-on-static-TURN-secret checks).

## DEFERRED
- `scripts/check-dep-direction.sh` — the bespoke dependency-direction graph gate. NOT needed while Rust is the only language (Cargo's compile-time cycle ban + cargo-deny cover it for free). Introduced in **Phase 3** with the TS side, when lateral cycles actually become possible.
- `scripts/gen-proto.sh` — **Phase 2** (`buf generate`, Rust/prost); **Phase 3** adds ts-proto.
- `release/` — versioning/changeset/release automation — introduced when there is a release cadence to automate (Phase 2+).
