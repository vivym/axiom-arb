.PHONY: fmt check test db-up db-down

fmt:
	cargo fmt --all

check:
	cargo check --workspace

test:
	cargo test --workspace

db-up:
	docker compose up -d postgres

db-down:
	docker compose down

