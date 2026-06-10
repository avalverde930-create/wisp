# Wisp

> Security-first remote desktop. View and control your own PC from any device, on hostile networks, with keys only you hold — self-host it or let us, your choice.

A new, security-first competitor to Duet Remote Desktop / Parsec / RustDesk / TeamViewer / AnyDesk. Near-term host: **Windows 11 Pro**. Long-term: hosts on Windows/macOS/Linux; clients on desktop, iOS, Android, and web.

## The two non-negotiables
1. **Safe & secure above all.** The host **never opens an inbound public port** — it dials *out* to a rendezvous broker (Phase 2+; the MVP is LAN-only). End-to-end encryption the relay cannot read. Assume hostile networks; the relay is the enemy.
2. **Scalable from day one.** One shared Rust core, thin platform shells, stateless control/data planes. The solo-VPS deployment and the SaaS fleet are *the same code*. **Scalability is delivered by the architecture (this repo's ADRs + a few load-bearing seams), NOT by scaffolding every directory on commit 1.**

## What exists NOW vs what is a reserved slot
This repo deliberately starts THIN. On commit 1 only the Phase 0–1 surface exists: `core/` (one crate), `host/`, `client/`, `spikes/`, `tools/`, `docs/`, `infra/docker`, `tests/`. The full north-star tree (`proto/`, split `core/*` crates, `services/*`, `bindings/*`, `clients-native/*`, `apps/*`, `packages/*`, `infra/{terraform,k8s,ansible}`) is a **reserved slot documented in `docs/adr/0001` and `CONTRIBUTING.md`** — created only when its phase forces it. An empty dir with a README is a liability, not scaffolding.

## Repo map (current)
```
core/      ONE Rust crate; modules: wire, codec, transport, framing, crypto, channel,
           identity, known_hosts, trust, session, media, audit
host/      host-core (lib) + host-windows (bin) [+ host-windows-helper in Phase 2]
client/    the single desktop dogfood client (winit + wgpu)
spikes/    throwaway Phase-0 de-risking experiments
tools/     dev scripts (bootstrap, CI helpers)
tests/     cross-cutting integration tests (unit tests stay in-crate)
infra/     docker/ (Tier-0 Compose bundle; populated in Phase 2)
docs/      ARCHITECTURE / SECURITY / ROADMAP / TECH-STACK / PLATFORM-MATRIX + adr/ + security/
```

## Quickstart
```bash
# Prereqs: Rust (see rust-toolchain.toml), just. (Node/pnpm NOT needed until Phase 3.)
just bootstrap      # cargo fetch
just build          # cargo build --workspace
just test           # cargo test + integration tests
just lint           # fmt + clippy + cargo-deny
just dev-host       # run the Windows host agent (Phase 1+)
just dev-client     # run the desktop client (Phase 1+)
```

## Documentation
- System design & boundary rules -> `docs/ARCHITECTURE.md`
- Threat model & disclosure policy -> `docs/SECURITY.md`
- Phased roadmap, solo timelines & exit criteria -> `docs/ROADMAP.md`
- Stack & build-vs-leverage rationale -> `docs/TECH-STACK.md`
- Supported platforms & capability degradation -> `docs/PLATFORM-MATRIX.md`
- How to contribute without breaking the DAG (and how to un-defer a slot) -> `CONTRIBUTING.md`
- Decision records -> `docs/adr/`

## License
See `LICENSE`. Note: this project does **not** fork RustDesk (AGPL-3.0); it uses the hbbs/hbbr rendezvous+relay topology only as a clean-room reference.
