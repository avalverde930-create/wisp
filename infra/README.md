# infra/

Infrastructure-as-code. Deploys `services/` (Phase 2+); contains no app logic. Secrets sourced from a manager/KMS, NEVER the tree.

## What exists now
**Almost nothing.** The MVP is LAN-only with zero backend. This dir reserves the `docker/` slot for the Tier-0 Compose bundle, which is populated in **Phase 2** (first deploy). `terraform/`, `k8s/`, and `ansible/` are DEFERRED (their mere presence on day one is an attractive nuisance) — `k8s/`+`ansible/` would contradict the plan's own 'never start on K8s' rule.

## Growth tiers (no re-architecture)
- **Tier 0 (solo/self-host, ~$25-50/mo, Phase 2):** `docker/` + `docker-compose.yml` on one VPS — Caddy (TLS, the edge — NOT a separate api-gateway) + signaling + (v1.0) identity + coturn/eturnal + Postgres + Redis. Nightly pg_dump -> restic -> object storage. Services may run as ONE binary behind feature flags here.
- **Tier 1 (first paying users):** managed Postgres (PITR); split the relay onto its own bandwidth-sized box; managed Redis. **`terraform/` starts here.**
- **Tier 2 (SaaS, multi-region):** stateless signaling fleet behind an LB + Redis pub/sub; regional relay fleet (GeoDNS/Anycast); managed container platform (Fly.io/Cloud Run/ECS) BEFORE Kubernetes; introduce `api-gateway`. **`k8s/`+`ansible/` start here.**

## Cost lever
Relay egress dominates. Treat the P2P-vs-relay ratio as a first-class SLI; IPv6-first + good ICE keep sessions direct (free).
