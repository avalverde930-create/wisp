# core/

Shared Rust domain logic — the trunk of the dependency DAG. Imports the wire module only (in MVP) / `protocol-types` (Phase 2+); **never** imports an app, the host, or a service.

## MVP layout: ONE crate, these modules (src/*.rs)
- `wire` — hand-written wire structs (the MVP single source of truth; replaced by generated `proto/` types in Phase 2).
- `crypto` — the ONLY place crypto primitives live: Noise handshake (XX first contact -> cached IK reconnect), key derivation. Audited as a unit, **fuzzed from Phase 0**. Crypto-grade CODEOWNERS.
- `audit` — append-only, hash-chained local audit log (named security asset; integrity model easy to get subtly wrong). Crypto-grade CODEOWNERS alongside crypto.
- `identity` — device keypair, secure-element bindings, trust store, revocation, **reserved slot for the offline recovery code**.
- `transport` — the quinn-based native data plane; packet-parsing path is a designated cargo-fuzz target. (`MediaTransport` becomes a trait when webrtc-rs joins in Phase 3.)
- `session` — pairing + consent state machine, deny-by-default capability model.
- `media` — codec negotiation + the capture/encode/input pipeline (concrete WgcSource/NvencEncoder/SendInput live host-side).

## Concrete before trait
Do NOT write FrameSource/VideoEncoder/MediaTransport/InputSink as traits before a concrete impl exists. Extract each trait when the SECOND impl forces it (x264 SoftwareEncoder is the first trait-forcing moment). The traits are the documented TARGET in docs/ARCHITECTURE.md.

## Planned crate-split (Phase 2-3)
`protocol-types <- crypto <- identity <- session <- transport <- media <- telemetry`, plus `audit` under crypto-grade ownership. Cargo enforces acyclicity at compile time.

## Dependency rules
Leaf-ward only. A core module/crate that reaches host/service/app code fails cargo-deny + (Phase 3) the dependency-direction gate.
