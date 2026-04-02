.PHONY: help build build-release build-ebpf ebpf-standalone build-core build-collector build-storage build-web build-tui build-daemon test test-all check fmt fmt-check lint clean install-tools ebpf-check dev-setup dev ci all

# Default target
help:
	@echo "Heimwatch Build System"
	@echo ""
	@echo "Build Targets:"
	@echo "  make build              - Build all crates (debug mode)"
	@echo "  make build-release      - Build all crates (release mode, optimized)"
	@echo "  make build-ebpf         - Check eBPF kernel-space programs"
	@echo "  make ebpf-standalone    - Build eBPF programs with BPF target (full build)"
	@echo "  make build-core         - Build heimwatch-core crate"
	@echo "  make build-collector    - Build heimwatch-collector crate"
	@echo "  make build-storage      - Build heimwatch-storage crate"
	@echo "  make build-web          - Build heimwatch-web crate"
	@echo "  make build-tui          - Build heimwatch-tui crate"
	@echo "  make build-daemon       - Build heimwatch-daemon crate"
	@echo ""
	@echo "Testing & Quality:"
	@echo "  make test               - Run tests for all crates"
	@echo "  make test-all           - Run all tests with verbose output"
	@echo "  make check              - Check compilation without building"
	@echo "  make fmt                - Format all code"
	@echo "  make fmt-check          - Check code formatting"
	@echo "  make lint               - Run clippy linter"
	@echo "  make ebpf-check         - Check eBPF code compilation"
	@echo ""
	@echo "Maintenance:"
	@echo "  make install-tools      - Install required build tools (nightly, bpf-linker)"
	@echo "  make clean              - Remove build artifacts"
	@echo ""
	@echo "Convenience (Multi-step):"
	@echo "  make dev-setup          - Install tools + smoke test (best for new devs)"
	@echo "  make dev                - Quick checks: check + fmt-check + lint + test"
	@echo "  make all                - Full pipeline: fmt + lint + test + build-release"
	@echo "  make ci                 - CI pipeline: fmt-check + lint + test + ebpf-check + build"
	@echo "  make code-quality       - Does basic code quality checks: fmt + lint+ check + ebpf-check"
	@echo ""

# Build targets
build:
	cargo build

build-release:
	cargo build --release

build-ebpf:
	cargo +nightly check --manifest-path crates/heimwatch-ebpf/Cargo.toml

build-core:
	cargo build -p heimwatch-core

build-collector:
	cargo build -p heimwatch-collector

build-storage:
	cargo build -p heimwatch-storage

build-web:
	cargo build -p heimwatch-web

build-tui:
	cargo build -p heimwatch-tui

build-daemon:
	cargo build -p heimwatch-daemon

ebpf-standalone:
	cargo +nightly build --target bpf-unknown-unknown --manifest-path crates/heimwatch-ebpf/Cargo.toml

# Testing targets
test:
	cargo test --workspace --lib

test-all:
	cargo test --workspace --verbose

check:
	cargo check

ebpf-check:
	cargo +nightly check --manifest-path crates/heimwatch-ebpf/Cargo.toml

# Code quality targets
fmt:
	cargo fmt

fmt-check:
	cargo fmt -- --check

lint:
	cargo clippy -- -D warnings

# Installation of tools
install-tools:
	@echo "Installing build tools..."
	rustup toolchain install nightly --component rust-src
	cargo install bpf-linker
	@echo "Build tools installed successfully!"

# Cleanup
clean:
	cargo clean
	rm -rf crates/heimwatch-ebpf-common/target
	rm -rf crates/heimwatch-ebpf/target

# Convenience targets
all: fmt lint test build-release
	@echo "✓ All checks passed!"

dev: check fmt-check lint test
	@echo "✓ Development checks passed!"

dev-setup: install-tools check fmt-check lint test
	@echo "✓ Development environment ready!"

ci: fmt-check lint test build-ebpf build
	@echo "✓ CI checks passed!"

code-quality: fmt lint check ebpf-check
	@echo "✓ Quality checks passed!"
