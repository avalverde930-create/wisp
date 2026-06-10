# 0001. Monorepo over polyrepo + Rust core + deferred-slot reservation

- **Status:** Accepted
- **Date:** 2026-06-09

## Context
The host agent, every client, and the backend all speak one wire protocol and reuse one core. We are a solo/small team that must scale without re-architecting. Two failure modes to avoid: (a) host/client protocol drift; (b) memory-corruption RCE in a network-facing daemon. The adversarial review found the original tree instantiated ~30 directories (~20 empty README-bearing) on commit 1 — a maintenance/attention tax that obscures the 3 files that matter this week.

## Decision
1. **Single polyglot monorepo** — but the day-one surface is ONLY the Phase 0-1 set: `core/` (ONE crate), `host/`, `client/`, `spikes/`, `tools/`, `docs/`, `infra/docker`, `tests/`.
2. **The full north-star tree is a RESERVED SLOT documented here**, created per phase (see the deferred-slot ledger in docs/ROADMAP.md): proto/+buf, the split core/* crates, services/*, bindings/*, clients-native/*, apps/* (Tauri/web/site), packages/*, infra/{terraform,k8s,ansible}, tools/release, tests/{e2e,load}. An empty dir with a README is a liability, not scaffolding.
3. **Rust is the language of the core, host, services, FFI surfaces.** Memory safety on a hostile-network workload is the security thesis; the wire module is shared host<->client<->server.
4. **Reserved slots (the DAG, in words):** `wire/proto -> core -> {host, client, services, bindings, apps}`. core/crypto and core/audit are crypto-grade-owned from day one.

## Consequences
- A protocol change + all consumers land in one atomic, compiler-verified PR; security fixes hit host + all clients + backend at once.
- The solo dev maintains ~8 dirs, not ~30; the structure is discovered as implementations force it.
- We deliberately do NOT fork RustDesk (AGPL-3.0).
- Enforcement machinery (check-dep-direction.sh, CODEOWNERS, Turborepo tags) is re-introduced when the graph/team grows, not before.
