# Memory Usage Tracking via Polling Snapshots

## Overview

This document outlines memory usage tracking per-process using periodic polling of `/proc` snapshots. Unlike CPU and disk I/O (which are event-driven in eBPF), memory is a point-in-time metric that changes gradually—making polling the most efficient and maintainable approach.

---

## Why Polling Instead of eBPF?

**Why not eBPF for memory:**
- Not event-driven — memory allocations are too frequent to hook efficiently (millions per second in some apps)
- Complex attribution — kernel memory vs. userspace, shared pages, swap all complicate per-process accounting
- Memory snapshots are sufficient — memory changes gradually; no need for continuous tracking
- eBPF would add overhead with minimal benefit

**Why polling is ideal:**
- Memory is a **stable metric** — changes on second-to-minute timescales
- Direct read from `/proc/[pid]/status` — one small syscall per process
- No kernel state to maintain — no BPF maps, no ring buffers
- Easy to understand and modify — simple synchronous reads
- Low overhead — selective polling (5-10 second intervals)

---

## Technical Approach

### Data Sources

**Primary source: `/proc/[pid]/status`**
```
VmPeak:    13112 kB     # Peak virtual memory
VmSize:    13112 kB     # Current virtual memory (VSZ)
VmLck:         0 kB     # Locked memory
VmPin:         0 kB     # Pinned memory
VmHWM:      2720 kB     # Peak RSS
VmRSS:      2720 kB     # Resident set size (physical memory)
VmData:     1204 kB     # Data segment
VmStk:       140 kB     # Stack segment
VmExe:        20 kB     # Code segment
VmLib:      2104 kB     # Shared library memory
VmSwap:        0 kB     # Swap usage
```

**Optional secondary source: `/proc/[pid]/smaps` or `/proc/[pid]/smaps_rollup`**
- PSS (Proportional Set Size) — more accurate for shared memory
- RSS_File, RSS_Shmem — breakdown by type
- Higher overhead; only read if needed

### Polling Strategy

**Recommended cadence:**
- **Poll interval**: 5-10 seconds
- **Frequency**: Slower than CPU (stable metric) but faster than infrequent updates
- **Selective**: Only read running processes, skip exited PIDs

**Rationale:**
- Memory changes gradually (seconds to minutes)
- 5-10s interval catches meaningful changes without excessive I/O
- Matches app-level monitoring granularity

---

## Data Structures

### Userspace Representation

```rust
#[derive(Debug, Clone)]
pub struct ProcessMemory {
    pub pid: u32,
    pub app_name: String,
    pub rss_bytes: u64,      // Physical memory (RAM)
    pub vms_bytes: u64,      // Virtual memory size
    pub swap_bytes: u64,     // Swap usage
    pub timestamp_sec: u64,
}

#[derive(Debug, Clone)]
pub struct AppMemoryUsage {
    pub app_name: String,
    pub rss_bytes: u64,      // Aggregated across all processes
    pub vms_bytes: u64,
    pub swap_bytes: u64,
    pub process_count: u32,  // Number of processes for this app
}
```

### Storage Schema

```rust
// In heimwatch-storage
pub struct MemoryDataPoint {
    pub timestamp: u64,
    pub app_name: String,
    pub rss_mb: f64,
    pub vms_mb: f64,
    pub swap_mb: f64,
}
```

---

## Collector Implementation

### File Structure

```
crates/heimwatch-collector/
├── src/
│   ├── lib.rs
│   └── linux/
│       ├── cpu.rs
│       ├── disk.rs
│       ├── network.rs
│       └── memory.rs      // New: memory polling collector
```

### Pseudocode: MemoryCollector

```rust
use procfs::process::Process;

pub struct MemoryCollector {
    last_snapshot: HashMap<String, AppMemoryUsage>,
}

impl MemoryCollector {
    pub async fn collect(&mut self) -> Result<HashMap<String, AppMemoryUsage>> {
        let mut current_stats: HashMap<String, AppMemoryUsage> = HashMap::new();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        // Get all running PIDs (e.g., via /proc or system call)
        let pids = self.get_running_pids()?;

        for pid in pids {
            // Read memory info from /proc/[pid]/status
            match Process::new(pid) {
                Ok(proc) => {
                    if let Ok(status) = proc.status() {
                        // Extract memory fields (in KB from /proc, convert to bytes)
                        let rss_bytes = status.vm_rss.unwrap_or(0) * 1024;
                        let vms_bytes = status.vm_size.unwrap_or(0) * 1024;
                        let swap_bytes = status.vm_swap.unwrap_or(0) * 1024;

                        // Resolve PID to app name
                        if let Ok(app_name) = self.resolve_pid_to_app(pid) {
                            current_stats
                                .entry(app_name)
                                .or_insert_with(AppMemoryUsage::default)
                                .add_process_memory(rss_bytes, vms_bytes, swap_bytes);
                        }
                    }
                }
                Err(_) => {
                    // PID may have exited between listing and reading
                    continue;
                }
            }
        }

        self.last_snapshot = current_stats.clone();
        Ok(current_stats)
    }

    fn get_running_pids(&self) -> Result<Vec<u32>> {
        // List /proc directory and extract numeric directories
        // Or use system crate to enumerate processes
    }

    fn resolve_pid_to_app(&self, pid: u32) -> Result<String> {
        // Read /proc/[pid]/comm or /proc/[pid]/cmdline
        // Map to app name (same as CPU/disk tracking)
    }
}
```

### Integration Points

1. **`heimwatch-collector/src/linux/memory.rs`** — MemoryCollector implementation
2. **`heimwatch-core/src/collector.rs`** — Add `MemoryDataPoint` to collector trait
3. **`heimwatch-storage/src/lib.rs`** — Add memory time-series schema
4. **`heimwatch-collector/src/scheduler.rs`** — Schedule memory collection every 5-10s

---

## Implementation Steps

### Phase 1: Polling Collector
1. Create `MemoryCollector` in `heimwatch-collector`
2. Read `/proc/[pid]/status` for each running process
3. Translate PID → app name (reuse logic from CPU/disk collectors)
4. Aggregate by app name
5. Test with various process types (long-lived, short-lived, multi-threaded)

### Phase 2: Storage & Aggregation
1. Add memory metrics to storage schema in `heimwatch-storage`
2. Store snapshots at 5-10 second intervals
3. Aggregate into 1m, 5m, 1h buckets for dashboard

### Phase 3: Dashboard Integration
1. Display memory usage graphs per-app
2. Show top memory consumers
3. Track memory trends (growing/stable/shrinking)
4. Optional: per-process breakdown

### Phase 4: Advanced Metrics
1. PSS (Proportional Set Size) from `/proc/smaps` for more accurate shared memory accounting
2. Memory pressure indicators (OOM risk)
3. Per-app memory growth rate warnings

---

## Polling Interval Considerations

| Interval | Use Case | Trade-off |
|----------|----------|-----------|
| **1-2s** | Real-time dashboards, OOM detection | Higher syscall overhead (~10/sec) |
| **5-10s** | Normal monitoring, app-level tracking | Good balance (1-2 syscalls/sec) |
| **30s+** | Historical archival only, low-power systems | May miss transient spikes |

**Recommendation**: **5-10 seconds** for heimwatch
- Captures meaningful memory changes (app startup, memory leaks)
- Minimal syscall overhead (<0.1% CPU)
- Aligns with other collector frequencies (network, disk at 1-2s; CPU at 5s)

---

## Performance Characteristics

| Metric | Value | Notes |
|--------|-------|-------|
| Per-process read time | ~0.2ms | One `/proc/[pid]/status` read |
| Total overhead (100 processes, 10s interval) | <0.1% CPU | Negligible compared to disk/CPU tracking |
| Memory footprint (collector state) | ~10KB | Stores last snapshot for 100+ apps |
| Syscall count | 1 per process per poll | Similar to `/proc` read pattern |

**Example**: Monitoring 500 processes with 10-second polling interval:
- 500 `/proc` reads every 10 seconds = 50 reads/sec
- Each read ~0.2ms = ~10ms total per cycle
- 10ms per 10s = 0.1% CPU overhead

---

## Challenges & Solutions

### Challenge 1: PID Recycling
**Problem:** PID reused after process exits.

**Solution (Same as CPU/Disk):**
- Aggregate by app name, not PID
- App name persists; PID recycling is transparent
- Archive historical stats by app name when process exits

### Challenge 2: Multi-Process Apps
**Problem:** Apps with multiple processes (chrome, firefox) show memory per-process.

**Solution:**
- Aggregate all processes with same app name
- Store process count for reference
- Display aggregated memory per app

### Challenge 3: Shared Memory & Page Cache
**Problem:** RSS counts shared pages toward both processes; doesn't reflect true memory usage.

**Solution (Current):**
- RSS is what the kernel reports; it's a reasonable per-process metric
- For more accuracy, read PSS from `/proc/smaps` (slower, ~5ms per process)
- Trade-off: RSS (fast, simple) vs. PSS (accurate but slower)
- **Recommendation**: Use RSS by default; offer PSS as optional detailed mode

### Challenge 4: Memory Snapshots Aren't Continuous
**Problem:** Short-lived memory spikes between polls may be missed.

**Solution:**
- 5-10s interval catches most spikes (apps don't allocate/free in microseconds)
- If you need spike detection, use `/proc/pressure/memory` for system-wide pressure
- For OOM prevention, monitor for sustained high memory, not transient spikes

### Challenge 5: Reading `/proc` While Processes Change
**Problem:** Process exits between listing PIDs and reading its status.

**Solution:**
- Wrap Process::status() in error handling
- Skip PIDs that disappear (process exited)
- This is normal and expected behavior

---

## Comparison: Memory Tracking Methods

| Method | Accuracy | Overhead | Complexity |
|--------|----------|----------|-----------|
| `/proc/[pid]/status` (RSS) | Good | Very low (<0.1%) | Low |
| `/proc/[pid]/smaps` (PSS) | Excellent | Higher (~5ms/process) | Medium |
| eBPF memory tracking | Variable | High (overhead not justified) | Very high |
| `sysinfo` crate | Medium | Low-Medium | Very low |

**Recommendation**: Start with `/proc/status` (RSS); upgrade to PSS if accuracy becomes critical.

---

## Integration with Other Collectors

```
Polling Schedule
────────────────────────────────────────
Time   0s        1s        2s        3s   ...
       │         │         │         │
CPU    │         ●         │         ●    (every 1-5s)
Disk   ●         ●         ●         ●    (every 1-2s)
Network●         ●         ●         ●    (every 1-2s)
Memory │         │    ●    │         │    (every 5-10s)

● = collection event
```

All collectors run independently with configurable intervals. Memory can lag others without impact.

---

## References

- `/proc` filesystem documentation: https://man7.org/linux/man-pages/man5/proc.5.html
- procfs Rust crate: https://docs.rs/procfs/latest/procfs/
- Linux memory management: https://www.kernel.org/doc/html/latest/admin-guide/mm/index.html
- PSS explanation: https://lwn.net/Articles/492814/
- Memory pressure stall information: https://www.kernel.org/doc/html/latest/accounting/psi.html

---

## Next Steps

1. Review this plan with the team
2. Implement Phase 1 (basic MemoryCollector with RSS)
3. Test with multi-process apps (browsers, servers)
4. Benchmark syscall overhead
5. Integrate into scheduler with 5-10s polling
6. Consider PSS mode for Phase 4 (if accuracy becomes a requirement)

---

*Document created as part of heimwatch memory, CPU, and disk tracking architecture planning.*
