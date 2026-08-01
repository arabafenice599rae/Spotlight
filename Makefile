# Spotlight — common developer tasks. Run `make help` for the list.
.DEFAULT_GOAL := help
.PHONY: help build test test-vetrina test-litesvm client fmt fmt-check keys-sync verify clean

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN{FS=":.*?## "}{printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}'

build: ## Build the on-chain program (-> target/deploy/vetrina.so)
	anchor build

test: test-vetrina test-litesvm ## Run all Rust tests

test-vetrina: ## Property tests P1–P4 + edge cases (no artifact needed)
	cargo test -p vetrina

test-litesvm: ## litesvm integration suite (needs `make build` first)
	cd tests-litesvm && cargo test -- --nocapture

client: ## Install + typecheck the TypeScript client
	cd client && npm install && npm run typecheck

fmt: ## Format Rust sources
	cargo fmt --all

fmt-check: ## Check Rust formatting
	cargo fmt --all -- --check

keys-sync: ## Sync declare_id with the program keypair
	anchor keys sync

verify: ## Reproducible build via solana-verify (Docker); see SECURITY.md
	solana-verify build --library-name vetrina

clean: ## Remove build artifacts
	cargo clean
	rm -rf tests-litesvm/target client/node_modules
