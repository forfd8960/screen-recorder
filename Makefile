.PHONY: build run run-debug test lint fmt fmt-check clean audit help

# Default log level when running
RUST_LOG ?= screen_recorder=info

##@ Building

build: ## Build (dev profile)
	cargo build

build-release: ## Build (release profile)
	cargo build --release

##@ Running

run: build ## Build dev binary and launch the app
	RUST_LOG=$(RUST_LOG) ./target/debug/screen-recorder

run-release: build-release ## Build release binary and launch the app
	RUST_LOG=$(RUST_LOG) ./target/release/screen-recorder

run-debug: ## Run with debug-level logs
	RUST_LOG=screen_recorder=debug cargo run

##@ Testing & Linting

test: ## Compile and run all tests
	cargo test

test-compile: ## Compile tests without running (useful when Swift runtime is absent)
	cargo test --no-run

lint: ## Run clippy on all targets
	cargo clippy --all-targets --all-features -- -D warnings

fmt: ## Format all source files
	cargo fmt

fmt-check: ## Check formatting without modifying files
	cargo fmt --check

##@ Maintenance

clean: ## Remove build artifacts
	cargo clean

audit: ## Run cargo-audit for known vulnerability advisories
	cargo audit

##@ Help

help: ## Show this help message
	@awk 'BEGIN {FS = ":.*##"; printf "\nUsage:\n  make \033[36m<target>\033[0m\n"} \
	/^[a-zA-Z_-]+:.*?##/ { printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2 } \
	/^##@/ { printf "\n\033[1m%s\033[0m\n", substr($$0, 5) }' $(MAKEFILE_LIST)
