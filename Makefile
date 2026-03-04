.PHONY: build run run-debug test lint fmt fmt-check clean audit sign-debug sign-release help

# Default log level when running
RUST_LOG ?= screen_recorder=info

# Stable bundle ID for TCC tracking — must not change across recompiles
BUNDLE_ID = com.forfd8960.screen-recorder

##@ Building

build: ## Build (dev profile) and ad-hoc sign
	cargo build
	codesign --sign - --identifier "$(BUNDLE_ID)" --force target/debug/screen-recorder

build-release: ## Build (release profile) and ad-hoc sign
	cargo build --release
	codesign --sign - --identifier "$(BUNDLE_ID)" --force target/release/screen-recorder

##@ Running

run: build ## Build dev binary and launch the app
	RUST_LOG=$(RUST_LOG) ./target/debug/screen-recorder

run-release: build-release ## Build release binary and launch the app
	RUST_LOG=$(RUST_LOG) ./target/release/screen-recorder

run-debug: ## Run with debug-level logs (dev build)
	RUST_LOG=screen_recorder=debug $(MAKE) run

##@ Signing

sign-debug: ## Re-sign the dev binary without rebuilding
	codesign --sign - --identifier "$(BUNDLE_ID)" --force target/debug/screen-recorder

sign-release: ## Re-sign the release binary without rebuilding
	codesign --sign - --identifier "$(BUNDLE_ID)" --force target/release/screen-recorder

reset-tcc: ## Reset ScreenCapture TCC — run once after first signing, then re-grant in System Settings
	tccutil reset ScreenCapture
	@echo "TCC reset. Relaunch the app — macOS will prompt for Screen Recording permission."

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
