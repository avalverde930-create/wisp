# Top-level command surface. MVP is Rust-only; TS + proto-gen targets arrive with their phase.

bootstrap:
    cargo fetch

build:
    cargo build --workspace

test:
    cargo test --workspace

lint:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo deny check

fuzz:
    # Phase 0+: wire-parser + Noise-handshake fuzz targets
    cargo +nightly fuzz run wire_parser -- -max_total_time=60

dev-host:
    cargo run -p host-windows

dev-client:
    cargo run -p client

# DEFERRED (uncomment when the phase arrives):
# gen:            # Phase 2: buf generate (Rust/prost). Phase 3: + ts-proto.
#     buf generate
# dev-web:        # Phase 3
#     pnpm --filter @wisp/web dev
