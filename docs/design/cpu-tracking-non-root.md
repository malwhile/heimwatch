# Tracking CPU Usage Per Process in Rust

## Overview

This document outlines methods for tracking CPU usage per process in Rust, including recommended crates, performance characteristics, and permission requirements.

---

## Recommended Crates and APIs

### 1. sysinfo (Recommended for Most Use Cases)

**Purpose:** Cross-platform system information library with per-process CPU tracking

**Key Features:**
- Reads from `/proc/[pid]/stat` on Linux
- Provides `cpu_usage()` method returning percentage per core
- Supports Windows, macOS, and Linux
- Automatic caching and refresh mechanisms

**Usage Pattern:**
\`\`\`rust
use sysinfo::{System, ProcessExt, SystemExt};

let mut system = System::new();
system.refresh_processes();

// Need to refresh twice for accurate CPU percentage
std::thread::sleep(std::time::Duration::from_millis(500));
system.refresh_processes();

for (pid, process) in system.processes() {
    let cpu_usage = process.cpu_usage(); // Returns f32 percentage
}
\`\`\`

**API Reference:**
- `System::new()` - Initialize system monitor
- `refresh_processes()` - Update process information
- `Process::cpu_usage()` - Get CPU percentage (can exceed 100% on multi-core)
- `Process::total_cpu_usage()` - Get accumulated CPU time

**Important Note:** CPU usage calculation requires at least two refresh calls with a time interval between them. The percentage represents CPU time consumed relative to elapsed time.

---

### 2. procfs (Low-Level Direct Access)

**Purpose:** Direct `/proc` filesystem parsing for maximum control

**Key Features:**
- Reads `/proc/[pid]/stat` directly
- Exposes `utime` (user mode) and `stime` (kernel mode) fields
- Minimal abstraction layer
- Linux-only

**Usage Pattern:**
\`\`\`rust
use procfs::process::Process;

let process = Process::new(pid).unwrap();
let stat = process.stat().unwrap();

// Fields 14 and 15 in /proc/[pid]/stat
let utime = stat.utime;  // User mode CPU ticks
let stime = stat.stime;  // Kernel mode CPU ticks

// Convert to seconds using sysconf(_SC_CLK_TCK)
let clk_tck = procfs::ticks_per_second().unwrap();
let total_time = (utime + stime) as f64 / clk_tck as f64;
\`\`\`

**API Reference:**
- `Process::new(pid)` - Create process handle
- `Process::stat()` - Parse `/proc/[pid]/stat`
- `Stat::utime` - User mode CPU time in ticks
- `Stat::stime` - Kernel mode CPU time in ticks
- `ticks_per_second()` - Get system clock tick rate

---

### 3. heim (Async-Friendly Alternative)

**Purpose:** Async-compatible system monitoring

**Key Features:**
- Built on tokio for async operations
- Caches values between updates
- Lower per-call overhead than sysinfo for selective queries
- Cross-platform support

**Usage Pattern:**
\`\`\`rust
use heim::process;

let cpu = process::cpu().await?;
// Returns CPU usage percentage
\`\`\`

---

### 4. nix (Syscall-Based)

**Purpose:** Raw syscall access via `getrusage` and `times`

**Key Features:**
- Minimal overhead
- More boilerplate code required
- POSIX-compliant (works on Unix systems)

**Usage Pattern:**
\`\`\`rust
use nix::sys::resource::{getrusage, RUSAGE_SELF};

let usage = getrusage(RUSAGE_SELF).unwrap();
let user_time = usage.user_time();  // timeval structure
let system_time = usage.system_time();
\`\`\`

---

## Performance Overhead Analysis

### sysinfo Performance Characteristics

| Aspect | Cost | Notes |
|--------|------|-------|
| Initial refresh | High | Allocates all Process structs (~10-50ms for 100+ processes) |
| Subsequent refresh | Medium | Can selectively refresh specific processes |
| Per-call overhead | Low-Medium | Cached data reduces syscall frequency |
| Memory usage | Moderate | Stores full process list in memory |

**Optimization Tips:**
- Use `ProcessRefreshKind` to refresh only CPU data
- Cache the `System` instance and reuse across calls
- Increase refresh interval for less frequent monitoring

---

### procfs Performance Characteristics

| Aspect | Cost | Notes |
|--------|------|-------|
| Per-process read | High | Separate syscall for each `/proc/[pid]/stat` |
| Total overhead | Very High | "Massive system call overhead" when scanning many processes |
| Memory usage | Minimal | No caching, reads directly from filesystem |
| Best use case | Single process monitoring | Not ideal for full-system scans |

**Benchmark Finding:** procfs is approximately 7x slower than optimized alternatives when monitoring multiple processes due to unoptimized per-file access patterns.

---

### Comparative Summary

| Crate | Speed | Memory | Complexity | Best For |
|-------|-------|--------|------------|----------|
| sysinfo | Medium-High | Medium | Low | General-purpose monitoring |
| procfs | Low | Low | Medium | Single process, minimal deps |
| heim | Medium | Medium | Medium | Async applications |
| nix | High | Low | High | Minimal overhead, Unix-only |

---

## Permissions Required

### Standard Permissions

**Reading `/proc/[pid]/stat`:**
- **Process Owner:** Can read their own process information without special permissions
- **Root (UID 0):** Can read any process information
- **Other Users:** Restricted by kernel security settings

### Special Capabilities

| Capability | Purpose | Required For |
|------------|---------|--------------|
| `CAP_SYS_PTRACE` | Trace/process inspection | Reading other users' processes |
| `CAP_SYS_ADMIN` | Administrative access | Some restricted `/proc` entries |

### Linux Security Modules

**hidepid Mount Option:**
\`\`\`bash
# Check /proc mount options
mount | grep proc

# hidepid=0 - No restrictions (default)
# hidepid=1 - Cannot access other users' processes
# hidepid=2 - Cannot list /proc directory contents
\`\`\`

**Recommendation:** Run monitoring tools with appropriate privileges or as the target process owner. For containerized environments, ensure the container has necessary capabilities.

---

## Implementation Recommendations

### For Production Monitoring

**Use sysinfo with these configurations:**
\`\`\`rust
use sysinfo::{System, ProcessRefreshKind};

let mut system = System::new_with_specifics(
    ProcessRefreshKind::new().with_cpu()
);

// Refresh only CPU data to minimize overhead
system.refresh_processes_specifics(ProcessRefreshKind::new().with_cpu());
\`\`\`

### For High-Frequency Polling

**Use procfs for single process:**
- Lower memory footprint
- Direct access without abstraction overhead
- Acceptable for monitoring specific PIDs

### For Async Applications

**Use heim:**
- Native async/await support
- Built-in caching reduces redundant reads
- Integrates with tokio runtime

---

## Code Block Escaping Notes

All code examples above are presented in markdown code blocks. When embedding in documentation systems that require escaping:

- Use triple backticks with language identifier
- Escape dollar signs if template expansion is enabled
- Preserve indentation for Rust syntax highlighting

---

## References

- sysinfo crate: https://crates.io/crates/sysinfo
- procfs crate: https://docs.rs/procfs
- heim crate: https://crates.io/crates/heim
- Linux /proc documentation: https://man7.org/linux/man-pages/man5/proc.5.html
- sysinfo benchmarks: https://github.com/joshuarli/procfs-rust-benchmarks

---

*Document generated based on research from official crate documentation, community benchmarks, and Linux kernel documentation.*
