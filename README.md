# AxiomArb

## Bootstrap

1. Copy `.env.example` to `.env` and keep the defaults for local development.
2. Start Postgres with `make db-up`.
3. Run `cargo test -p config load_settings_requires_database_url_and_mode -- --exact` while the config crate is still being bootstrapped.
4. Run `cargo test -p config` and `cargo check --workspace` once the config contract is implemented.

## Workspace

This repository is a Rust workspace for the AxiomArb live engine. The first bootstrap stage defines the runtime env contract and a local Postgres service; later tasks will fill in the remaining crates.
