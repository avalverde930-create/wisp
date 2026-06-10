# Contributing

## Golden rule
Respect the dependency-direction law: `wire/proto -> core -> {host, client, services, bindings, apps}`. A PR that introduces a forbidden edge fails CI. See `docs/ARCHITECTURE.md` §5.

## Un-deferring a reserved slot
Most of the north-star tree is intentionally absent (see the deferred-slot ledger in `docs/ROADMAP.md`). To bring a slot to life: (1) confirm its phase has arrived; (2) read the slot's reservation note in `docs/adr/0001`; (3) create the directory + minimal manifest; (4) add the matching enforcement (e.g., when the first TS package lands, add Turborepo tags + the check-dep-direction gate). Do NOT pre-create empty dirs 'to be ready' — an empty dir with a README is a liability.

## Adding a Rust crate (when core/ splits)
1. Create under the correct top-level dir (core/, host/, services/, bindings/, crates/).
2. Add it to the root Cargo.toml [workspace] members.
3. Pin shared deps via [workspace.dependencies] (single-version policy).
4. Declare allowed edges; cargo deny check verifies (the bespoke graph gate arrives with the TS side).

## Changing the wire protocol
- **MVP:** edit `core/src/wire.rs` (hand-written). Host and client share it via the same crate.
- **Phase 2+:** edit `proto/srd/v1/*.proto`, run `just gen`. CI fails if generated code is stale or buf detects an unversioned breaking change. Breaking-change gate is RELAXED during Phase 0-1 spike velocity (there is no proto then) and STRICT from Phase 2.

## Conventions
- Branches: feat/, fix/, sec/, chore/.
- Commits: Conventional Commits.
- Rust: kebab-case dir = snake_case crate, product-prefixed where published (srd-core, srd-crypto). TS (Phase 3): scoped @srd/<name>.
- ADRs: NNNN-kebab-title.md, append-only, never renumbered.
- Source files follow language idiom, NOT the library's date-prefix naming scheme (codebases are exempt).

## Security
All crypto changes touch core/crypto only and require code-owner review; all audit-log changes touch core/audit only (same ownership). Never commit a real secret; pre-commit gitleaks/trufflehog runs on every commit and FAILS the build on any plaintext private key or TURN static-auth-secret.
