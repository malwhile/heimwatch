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

### Tracepoint Field Offset Portability

⚠️ **Current Limitation:** eBPF probes (CPU `sched_switch`, Disk `block_rq_issue`, Network `tcp_sendmsg`/`tcp_recvmsg`) use **hardcoded field offsets** to read tracepoint context data. These offsets can shift between kernel versions (though they're stable in 4.4–6.x).

**Verification before first deployment on a new system:**

```bash
# CPU: sched_switch offsets
cat /sys/kernel/debug/tracing/events/sched/sched_switch/format
# Expected: prev_pid@offset 24, next_pid@offset 56

# Disk: block_rq_issue offsets
cat /sys/kernel/debug/tracing/events/block/block_rq_issue/format
# Expected: nr_sector@offset 24, rwbs@offset 32, comm@offset 40

# Network: tcp_sendmsg / tcp_recvmsg (kprobes, no tracepoint)
# No offset verification needed; uses function arguments directly
```

If offsets don't match, update the hardcoded values in `bpf/heimwatch-ebpf/src/main.rs` and rebuild.

**Future improvement:** Use **CO-RE (Compile Once Run Everywhere)** with BTF to auto-detect offsets at runtime (aya + Linux 4.18+). See the CO-RE roadmap note below.

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
- **eBPF portability trade-off**: Currently uses hardcoded tracepoint offsets for simplicity; future refactor should adopt CO-RE (Compile Once Run Everywhere) with BTF for kernel-agnostic field discovery (Linux 4.18+)

## CO-RE Implementation (Offset Auto-Discovery)

**Status:** ✅ **Implemented for all tracepoint probes** (block_rq_issue, sched_switch)

**Problem (solved):** Hardcoded tracepoint field offsets break across kernel versions.

**Solution:** Use **CO-RE + BTF (BPF Type Format)** to auto-detect struct field offsets at load time.

**Implementation (disk probe, `bpf/heimwatch-ebpf/src/main.rs`):**

```rust
// Define struct with correct field layout
#[repr(C)]
struct BlockRqIssue {
    _common_type: u32,
    _common_flags: u32,
    _common_preempt_count: i32,
    _common_pid: i32,
    dev: u32,
    _pad1: u32,
    sector: u64,
    nr_sector: u32,
    _pad2: u32,
    rwbs: [u8; 8],
    comm: [u8; 16],
}

// Cast context pointer to struct (no hardcoded offsets)
let ptr = ctx.as_ptr() as *const BlockRqIssue;
let rq_issue = unsafe { core::ptr::read_unaligned(ptr) };

// Access fields by name; aya resolves offsets from kernel BTF at load time
if rq_issue.nr_sector == 0 { /* ... */ }
let is_read = rq_issue.rwbs[0] == b'R';
```

**How it works:**
1. **Compile time:** eBPF program defines struct with field order matching kernel's tracepoint
2. **Load time:** aya reads kernel's BTF (if available) and adjusts field offsets automatically
3. **Runtime:** Single binary works across all kernel versions (4.18+)

**Requirements:**
- Kernel 4.18+ with BTF support (`CONFIG_DEBUG_INFO_BTF=y`)
- LLVM 10+ (bpf-linker already uses LLVM 18) ✓
- No special rustup configuration needed (already in place)

**Benefits:**
- ✅ No hardcoded offsets (portable across kernel versions)
- ✅ No offset verification needed (auto-detected)
- ✅ Single compiled binary for all supported kernels
- ✅ Compile Once Run Everywhere (C.O.R.E.) principle

**Probes converted to CO-RE:**
- ✅ CPU probe (`trace_sched_switch` on `sched:sched_switch`)
- ✅ Disk probe (`trace_block_rq_issue` on `block:block_rq_issue`)

**Probes without CO-RE (by design):**
- Network probes (`trace_sendmsg`, `trace_recvmsg`) use kprobes/kretprobes, which monitor stable kernel function signatures, not tracepoint structs. No offset variation across kernel versions.

## Development Notes

- **Privacy-first principle**: All data remains local; no external telemetry or cloud sync
- **Rust best practices**: Use `cargo fmt`, `cargo clippy`, and comprehensive test coverage
- **Cross-platform mindset**: Code should be structured to support Linux, macOS, Windows, and BSD eventually
- **Current focus**: Phase 1 is scaffolding the workspace and architecture; Phase 2 begins Linux/Wayland data collection
