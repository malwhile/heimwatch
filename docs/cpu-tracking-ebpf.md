# CPU Tracking via eBPF

## Overview

This document outlines CPU usage tracking per-process using eBPF (extended Berkeley Packet Filter). This approach provides kernel-space monitoring with minimal userspace overhead, aligning with heimwatch's existing eBPF infrastructure for network traffic monitoring.

---

## Why eBPF for CPU Tracking?

**Advantages:**
- **Minimal overhead**: Kernel-space data collection avoids repeated `/proc` reads and context switching
- **Event-driven**: Tracks CPU time as events occur rather than polling
- **Low memory footprint**: Only stores aggregated data per process, not full process tables
- **Existing infrastructure**: Heimwatch already has eBPF build setup and kernel compatibility checks
- **Scalability**: Efficient even with hundreds of processes running

**Trade-offs:**
- Linux-only (consistent with network tracking via eBPF)
- Requires kernel 4.4+ (eBPF basics) or 5.8+ (preferred for BPF_PROG_TYPE_PERF_EVENT)
- Requires `CAP_BPF` and `CAP_PERFMON` (or `CAP_SYS_ADMIN` on older kernels)

---

## Technical Approach

### Data Collection Strategy

Use **performance counters** via `BPF_PROG_TYPE_PERF_EVENT` to track CPU cycles and instructions:

1. **Attach to `cycles` perf event** — fires on every N CPU cycles per-core
2. **Read process context** — eBPF program can access current PID/TID
3. **Aggregate in kernel-space BPF maps** — maintain running totals per PID
4. **Userspace reads periodically** — pull aggregated data (e.g., every 1-5 seconds)

### Data Structures

**Kernel-side BPF map (hash map):**
```rust
// Key: PID (u32)
// Value: { cpu_cycles: u64, instruction_count: u64, last_update_time: u64 }
struct CpuStats {
    cycles: u64,
    instructions: u64,
    timestamp_ns: u64,
}
```

**Userspace representation:**
```rust
pub struct ProcessCpuUsage {
    pub pid: u32,
    pub cycles_per_second: f64,
    pub cpu_time_ms: f64,
    pub sample_interval_ms: u64,
}
```

---

## eBPF Program Design

### File Structure

```
crates/heimwatch-ebpf/
├── src/
│   ├── lib.rs
│   ├── cpu.rs          // CPU tracking eBPF program
│   └── shared.rs       // Shared types (BPF + userspace)
```

### eBPF Program (`cpu.rs`)

```c
#include <uapi/linux/ptrace.h>
#include <linux/perf_event.h>

struct cpu_stat {
    u64 cycles;
    u64 instructions;
    u64 timestamp_ns;
};

BPF_HASH(cpu_stats, u32, struct cpu_stat);

// Attached to perf event: cpu-cycles
int track_cpu_cycles(struct bpf_perf_event_data *ctx) {
    u32 pid = bpf_get_current_pid_tgid() >> 32;
    
    struct cpu_stat *stat = cpu_stats.lookup_or_init(&pid, 0);
    if (stat) {
        __sync_fetch_and_add(&stat->cycles, 1);
        stat->timestamp_ns = bpf_ktime_get_ns();
    }
    
    return 0;
}
```

### Sampling Strategy

To minimize kernel overhead:
- **Sample interval**: Attach to perf event with `sample_freq = 1000` (1000 samples/second per core)
- **Aggregation**: BPF map aggregates automatically; userspace reads once per second
- **Ring buffer alternative**: Use `BPF_RINGBUF` for higher-frequency updates if needed

---

## Integration with Heimwatch

### Collector Architecture

Add to `heimwatch-collector`:

```rust
pub struct CpuCollector {
    perf_event: PerfEvent,
    cpu_prog: CpuProgram,
    stats_map: BpfHashMap<u32, CpuStats>,
}

impl CpuCollector {
    pub async fn collect(&self) -> Result<HashMap<String, ProcessCpuUsage>> {
        // Read stats_map, translate PID→app_name, aggregate and return CPU usage per app
    }
}
```

### Data Flow

```
Kernel                          Userspace
─────────────────────────────────────────────
[CPU cycles]                    
    │                           
    ├─→ perf event fired        
    │   (sample_freq = 1000)     
    │                           
    └─→ eBPF prog                
        └─→ BPF hash map         
            (by PID)             
                                ← heimwatch-collector (reads every 1-5s)
                                  ├─→ Translate PID → app name
                                  ├─→ Aggregate by app
                                  ├─→ Calculate deltas
                                  └─→ Store in sled
```

**Note**: By translating PID to app name in userspace, we automatically handle PID recycling—app name persists across process restarts.

### Integration Points

1. **`heimwatch-ebpf/src/cpu.rs`** — eBPF program source
2. **`heimwatch-collector/src/linux/cpu.rs`** — Linux-specific CPU collector
3. **`heimwatch-core/src/collector.rs`** — Add `CpuDataPoint` to collector trait
4. **`heimwatch-storage/src/lib.rs`** — Add CPU time-series schema

---

## Implementation Steps

### Phase 1: eBPF Program
1. Write `heimwatch-ebpf/src/cpu.rs` with perf event attachment
2. Define shared types in `heimwatch-ebpf-common`
3. Test BPF program loads and attaches on Linux 5.8+

### Phase 2: Userspace Collection
1. Create `CpuCollector` in `heimwatch-collector`
2. Read BPF map and calculate per-process deltas
3. Handle PID recycling (when a PID is reused after process exits)

### Phase 3: Storage & Aggregation
1. Add CPU metrics to storage schema
2. Aggregate into 1s, 5s, and longer intervals
3. Query interface for per-process CPU over time

### Phase 4: Dashboard Integration
1. Display CPU usage graphs per-process
2. Show top CPU consumers
3. Historical trends

---

## Kernel Version Compatibility

| Kernel | Feature | Status |
|--------|---------|--------|
| 4.4+ | Basic eBPF, BPF maps | ✅ Supported (older) |
| 5.0+ | BPF_PERF_EVENT | ✅ Supported |
| 5.8+ | BPF_RINGBUF, CAP_BPF/CAP_PERFMON | ✅ Recommended |
| 6.0+ | Additional perf optimizations | ✅ Optimal |

**Recommendation**: Target 5.8+ for capability support; fallback to `CAP_SYS_ADMIN` on older kernels.

---

## Permission Requirements

**At runtime**, heimwatch daemon needs:
```bash
# Modern kernels (5.8+)
CAP_BPF
CAP_PERFMON

# Older kernels (< 5.8)
CAP_SYS_ADMIN

# Verify with
getcap /path/to/heimwatch-daemon
```

**Build-time**: Standard eBPF toolchain (already set up):
- `rustup toolchain install nightly --component rust-src`
- `cargo install bpf-linker`

---

## Performance Characteristics

| Metric | Value | Notes |
|--------|-------|-------|
| CPU overhead per sample | <0.1% | Kernel-space aggregation |
| Memory per process | ~32 bytes | (2 × u64 + timestamp) |
| Userspace read latency | <1ms | HashMap lookup for ~100 processes |
| Typical polling interval | 1-5 seconds | Configurable via collector |

**Example**: Monitoring 500 processes with 1-second polling interval adds <0.5% CPU overhead.

---

## Challenges & Solutions

### Challenge 1: PID Recycling
**Problem:** After a process exits, its PID can be reused by a new process.

**Solution (Recommended):** Aggregate by app name instead of PID
- When userspace reads the BPF map, translate each PID → app name (binary, command, etc.)
- Accumulate CPU stats under app name, not PID
- This aligns with heimwatch's existing network monitoring approach
- Completely sidesteps PID recycling: app name persists even when underlying process terminates and PID is reused

**Implementation:**
```rust
// Pseudocode in CpuCollector::collect()
let mut app_stats: HashMap<String, ProcessCpuUsage> = HashMap::new();
for (pid, kernel_stats) in self.stats_map.iter() {
    if let Ok(app_name) = resolve_pid_to_app(pid) {
        app_stats.entry(app_name)
            .or_insert_default()
            .add_cpu_time(kernel_stats);
    }
}
// Return aggregated stats by app name
```

**Alternative (if per-PID granularity needed):**
- Record process exit events via `sched:sched_process_exit` tracepoint
- Archive stats before clearing the BPF map entry
- Include start time to distinguish process instances

### Challenge 2: Cross-Core Attribution
**Problem:** CPU cycles are per-core; how to attribute to multi-threaded processes?

**Solution:**
- Accumulate by PID (kernel does this automatically for all threads in a process)
- Record sample count separately if detailed per-thread breakdown is needed later

### Challenge 3: Frequency Scaling
**Problem:** CPU frequency varies; cycle count alone doesn't reflect actual time.

**Solution:**
- Use `cpu-clock` event (wall-clock time) instead of `cpu-cycles` for precise timing
- Or combine with `ref-cycles` (reference/unscaled cycles) for comparison

---

## Alternative Approaches Considered

### 1. **sched_switch tracepoint**
- Tracks context switches (when process loses CPU)
- Lower granularity than perf events
- Would require summing time deltas

### 2. **BPF_RINGBUF with streaming events**
- Higher resolution, but higher overhead
- Better for real-time dashboards
- Overkill for 1-5 second polling intervals

### 3. **/proc polling with nix syscalls**
- Zero overhead for self (own daemon), but minimal
- Covered in `cpu-tracking-non-root.md` as fallback

---

## References

- eBPF perf events: https://docs.kernel.org/bpf/
- libbpf documentation: https://github.com/libbpf/libbpf
- Linux perf events: https://man7.org/linux/man-pages/man2/perf_event_open.2.html
- Heimwatch eBPF setup: See CLAUDE.md (eBPF Development Setup)

---

## Next Steps

1. Review this plan with the team
2. Prototype eBPF program on Linux 5.8+ kernel
3. Implement Phase 1 and Phase 2
4. Benchmark overhead against target (<1% CPU for 500 processes)

---

*Document created as part of heimwatch Phase 1 architecture planning.*
