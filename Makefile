.PHONY: fmt clippy check test db-up db-down live-paper

fmt:
	cargo fmt --all

clippy:
	cargo clippy --workspace --all-targets

check:
	cargo check --workspace

test:
	cargo test --workspace

db-up:
	docker compose up -d postgres

db-down:
	docker compose down

live-paper:
	@echo "live-paper is a placeholder until the live app is implemented"
