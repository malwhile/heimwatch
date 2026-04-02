# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**Heimwatch** is a privacy-focused, cross-platform system monitoring daemon written in Rust. It tracks application usage, network traffic, power consumption, and system resources with an embedded database and dual interface (web dashboard + TUI).

See [README.md](README.md) for the full vision and roadmap.

## Workspace Structure

This is a Rust workspace with the following planned crates:

- **`heimwatch-core`**: Shared types, traits, and OS abstraction layers
- **`heimwatch-collector`**: OS-specific data collection (Network, Power, Focus, etc.)
- **`heimwatch-storage`**: Time-series data persistence using `sled`
- **`heimwatch-web`**: Lightweight Axum server serving the dashboard
- **`heimwatch-tui`**: Ratatui-based terminal interface
- **`heimwatch-daemon`**: Service management (systemd, launchd, etc.)

Currently in **Phase 1** (scaffolding). See README roadmap for implementation phases.

## Common Development Commands

```bash
# Create a new workspace crate
cargo init --lib crates/heimwatch-core
# or
cargo init --bin crates/heimwatch-daemon

# Build all crates
cargo build

# Run tests (all crates)
cargo test

# Run tests for a specific crate
cargo test -p heimwatch-core

# Run a specific binary
cargo run -p heimwatch-daemon

# Format code
cargo fmt

# Lint code
cargo clippy -- -D warnings

# Check compilation without building
cargo check

# Generate documentation
cargo doc --no-deps --open
```

## eBPF Development Setup

**⚠️ Linux Only**: eBPF is a Linux kernel feature. The `heimwatch-ebpf` and `heimwatch-ebpf-common` crates are excluded from the workspace and only compile on Linux.

- **On Linux**: Full eBPF setup required (see below)
- **On macOS/Windows**: `cargo build` works without eBPF; these crates are skipped
- Both platforms can run tests and use the collector (Linux with eBPF, macOS/Windows with stubs)

Network traffic monitoring uses eBPF (extended Berkeley Packet Filter) for kernel-space byte counting on Linux. The eBPF crates require additional toolchain setup (Linux developers only).

### One-Time Prerequisites (Linux only)

```bash
# Install nightly Rust toolchain with rust-src component
# Note: bpf-unknown-unknown is built from rust-src; no pre-built target exists
rustup toolchain install nightly --component rust-src

# Install bpf-linker (LLVM-based BPF linker; requires LLVM 18+)
cargo install bpf-linker
```

### Building eBPF Programs

The `heimwatch-collector` crate has a `build.rs` that automatically cross-compiles the eBPF program during `cargo build`. No manual compilation is needed; the above setup is only required once per development machine.

To verify the BPF program builds standalone:

```bash
cargo +nightly build --target bpf-unknown-unknown -p heimwatch-ebpf
```

The compiled ELF object is embedded in the user-space `heimwatch-collector` binary at build time.

### Troubleshooting eBPF Issues

If the daemon fails to attach BPF probes at runtime, use these diagnostics:

**Check kernel version:**
```bash
uname -r
# Should be 4.x or later (5.4+ recommended)
```

**Verify capabilities:**
```bash
getcap /path/to/heimwatch
# Should show: cap_bpf,cap_perfmon+ep
```

**Check kernel logs for BPF errors:**
```bash
dmesg | grep -i bpf
# Look for any "BPF" or "eBPF" errors
```

**View kernel trace buffer (eBPF warnings):**
```bash
cat /sys/kernel/debug/tracing/trace | grep NETWORK_STATS
# Shows if BPF map is full or other warnings
# (Enable with: echo 1 > /sys/kernel/debug/tracing/events/enable)
```

## GitHub Automation

The `gh-scripts/` directory contains scripts for managing GitHub project infrastructure:
- `heimwatch-issues.sh` — Bulk issue creation
- `heimwatch-labels.sh` — Label management
- `heimwatch-milestones.sh` — Milestone setup

These are one-time setup scripts; see their contents for usage.

## Key Architectural Decisions

- **Modular workspace**: Each crate has a clear responsibility to enable independent testing and potential future modularization
- **OS abstraction**: `heimwatch-core` provides traits that OS-specific implementations in `heimwatch-collector` conform to, enabling cross-platform support
- **Embedded storage**: Uses `sled` for local persistence (privacy-first, no cloud sync)
- **Dual interface**: Web dashboard (Axum + HTMX + Chart.js) and TUI (Ratatui) for different deployment scenarios

## Development Notes

- **Privacy-first principle**: All data remains local; no external telemetry or cloud sync
- **Rust best practices**: Use `cargo fmt`, `cargo clippy`, and comprehensive test coverage
- **Cross-platform mindset**: Code should be structured to support Linux, macOS, Windows, and BSD eventually
- **Current focus**: Phase 1 is scaffolding the workspace and architecture; Phase 2 begins Linux/Wayland data collection
