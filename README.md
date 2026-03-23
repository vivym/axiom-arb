# AxiomArb

## Bootstrap

1. Copy `.env.example` to `.env` and keep the defaults for local development.
2. Start Postgres with `make db-up`.
3. Run `cargo test -p config` and `cargo check --workspace` after the config contract is in place.

## Workspace

This repository is a Rust workspace for the AxiomArb live engine. The first bootstrap stage defines the runtime env contract and a local Postgres service; later tasks will fill in the remaining crates.
