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
