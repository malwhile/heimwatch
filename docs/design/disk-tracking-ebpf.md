# Disk I/O Tracking via eBPF

## Overview

This document outlines disk I/O tracking (reads, writes, IOPS) per-process using eBPF. This approach provides kernel-space monitoring of block device I/O with minimal userspace overhead, complementing the CPU and network tracking collectors in heimwatch.

---

## Why eBPF for Disk I/O?

**Advantages:**
- **Kernel-space capture**: Intercepts all I/O at block device layer, regardless of filesystem or syscall pattern
- **Minimal overhead**: eBPF tracepoints accumulate data; userspace just reads periodically
- **Syscall-agnostic**: Captures I/O from read/write/mmap/vectored I/O all in one place
- **Per-process attribution**: Block layer includes process context; no guessing
- **Low memory**: Aggregates bytes and counts in-kernel, not buffering raw I/O events
- **Consistent pattern**: Aligns with CPU and network tracking architecture

**Trade-offs:**
- Linux-only (consistent with CPU tracking)
- Requires kernel 4.4+ (block tracepoints available since 3.15+)
- Requires `CAP_BPF` and `CAP_PERFMON` (or `CAP_SYS_ADMIN` on older kernels)

---

## Technical Approach

### Block Device Tracepoints

Use **block device tracepoints** to track I/O events:

1. **`block:block_rq_issue`** — fired when I/O request is submitted to device
2. **`block:block_rq_complete`** — fired when I/O request completes
3. **Extract**: sector count, operation type (read/write), process context (PID)
4. **Aggregate in kernel-space BPF maps** — maintain read/write bytes per PID
5. **Userspace reads periodically** — pull aggregated data (e.g., every 1-2 seconds)

### Data Collection Strategy

Instead of sampling like CPU, we hook I/O request completions:
- Each I/O completion event increments counters for that PID
- Track both **bytes** (sectors × 512) and **IOPS** (operation count)
- Distinguish **read** vs **write** operations
- Optional: track by device (sda, nvme0n1, loop0, etc.) if needed

---

## Data Structures

### Kernel-side BPF Maps

**Primary map: read/write stats per PID**
```rust
struct DiskStats {
    bytes_read: u64,
    bytes_written: u64,
    read_ops: u64,
    write_ops: u64,
    timestamp_ns: u64,
}

BPF_HASH(disk_stats, u32, struct DiskStats);
```

**Optional: per-device breakdown**
```rust
struct DiskDeviceKey {
    pid: u32,
    device_major: u32,
    device_minor: u32,
}

BPF_HASH(disk_stats_by_device, struct DiskDeviceKey, struct DiskStats);
```

### Userspace Representation

```rust
pub struct AppDiskUsage {
    pub app_name: String,
    pub bytes_read_per_second: f64,
    pub bytes_written_per_second: f64,
    pub read_ops_per_second: f64,
    pub write_ops_per_second: f64,
}
```

---

## eBPF Program Design

### File Structure

```
crates/heimwatch-ebpf/
├── src/
│   ├── lib.rs
│   ├── cpu.rs
│   ├── disk.rs        // Disk I/O tracking (new)
│   └── shared.rs
```

### eBPF Program (`disk.rs`)

```c
#include <uapi/linux/ptrace.h>

struct disk_stat {
    u64 bytes_read;
    u64 bytes_written;
    u64 read_ops;
    u64 write_ops;
    u64 timestamp_ns;
};

BPF_HASH(disk_stats, u32, struct disk_stat);

// Attached to block:block_rq_complete tracepoint
// Fires when an I/O request completes
TRACEPOINT_PROBE(block, block_rq_complete) {
    u32 pid = bpf_get_current_pid_tgid() >> 32;
    
    struct disk_stat *stat = disk_stats.lookup_or_init(&pid, 0);
    if (!stat)
        return 0;
    
    u64 bytes = args->nr_sector * 512;  // Convert sectors to bytes
    
    // Determine if read or write operation
    // rwbs field: 'R' for read, 'W' for write
    // args->rwbs is a string-like field; we can check the first char
    if (args->op == 0) {  // REQ_OP_READ
        __sync_fetch_and_add(&stat->bytes_read, bytes);
        __sync_fetch_and_add(&stat->read_ops, 1);
    } else if (args->op == 1) {  // REQ_OP_WRITE
        __sync_fetch_and_add(&stat->bytes_written, bytes);
        __sync_fetch_and_add(&stat->write_ops, 1);
    }
    
    stat->timestamp_ns = bpf_ktime_get_ns();
    
    return 0;
}
```

### Tracepoint Alternatives

**`block:block_rq_issue`** vs **`block:block_rq_complete`**:
- **`block_rq_issue`**: Fires when request submitted; may still be in-flight
- **`block_rq_complete`**: Fires when request completes; accurate I/O count
- **Recommendation**: Use `block_rq_complete` for accuracy; if latency tracking is needed later, use both

---

## Integration with Heimwatch

### Collector Architecture

Add to `heimwatch-collector`:

```rust
pub struct DiskCollector {
    disk_prog: DiskProgram,
    stats_map: BpfHashMap<u32, DiskStats>,
    last_read: HashMap<String, DiskStats>,  // For calculating deltas
}

impl DiskCollector {
    pub async fn collect(&self) -> Result<HashMap<String, AppDiskUsage>> {
        // Read stats_map, translate PID→app_name
        // Calculate deltas (current - last_read)
        // Return per-app disk usage
    }
}
```

### Data Flow

```
Kernel                          Userspace
─────────────────────────────────────────────
[Block I/O events]              
    │                           
    ├─→ I/O request completes   
    │   on device               
    │                           
    └─→ block:block_rq_complete
        ├─→ eBPF prog            
        └─→ BPF hash map         
            (aggregates stats)   
                                ← heimwatch-collector (reads every 1-2s)
                                  ├─→ Translate PID → app name
                                  ├─→ Calculate bytes/ops per second
                                  ├─→ Aggregate by app
                                  └─→ Store in sled
```

### Integration Points

1. **`heimwatch-ebpf/src/disk.rs`** — eBPF program source
2. **`heimwatch-collector/src/linux/disk.rs`** — Linux-specific disk collector
3. **`heimwatch-core/src/collector.rs`** — Add `DiskDataPoint` to collector trait
4. **`heimwatch-storage/src/lib.rs`** — Add disk I/O time-series schema

---

## Implementation Steps

### Phase 1: eBPF Program
1. Write `heimwatch-ebpf/src/disk.rs` with tracepoint attachment
2. Define shared types in `heimwatch-ebpf-common`
3. Test on Linux with various I/O workloads (fio, dd, normal filesystem operations)

### Phase 2: Userspace Collection
1. Create `DiskCollector` in `heimwatch-collector`
2. Read BPF map and calculate deltas
3. Translate PID → app name and aggregate
4. Handle app restarts (when PID is recycled)

### Phase 3: Storage & Aggregation
1. Add disk metrics to storage schema
2. Aggregate into 1s, 5s, and longer intervals
3. Query interface for per-app disk I/O over time

### Phase 4: Dashboard Integration
1. Display disk I/O graphs per-app
2. Show top disk consumers
3. Distinguish read vs. write patterns
4. Optional: per-device breakdown

---

## Kernel Version Compatibility

| Kernel | Feature | Status |
|--------|---------|--------|
| 3.15+ | Block tracepoints | ✅ Supported |
| 4.4+ | BPF on tracepoints | ✅ Supported |
| 5.0+ | Enhanced tracepoint fields | ✅ Supported |
| 5.8+ | CAP_BPF/CAP_PERFMON | ✅ Recommended |
| 6.0+ | Additional optimizations | ✅ Optimal |

**Recommendation**: Target 4.4+ for broad compatibility; 5.8+ for cleaner capability handling.

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

**Build-time**: Standard eBPF toolchain (already set up).

---

## Performance Characteristics

| Metric | Value | Notes |
|--------|-------|-------|
| Overhead per I/O event | <0.1% | Kernel-space increment in BPF map |
| Memory per process | ~40 bytes | (4 × u64 + timestamp) |
| Userspace read latency | <1ms | HashMap lookup for ~100 processes |
| Typical polling interval | 1-2 seconds | Faster than CPU (bursty metric) |

**Example**: Heavy disk workload (1000 I/O ops/sec) with 50 processes monitoring adds <0.5% CPU overhead.

---

## Challenges & Solutions

### Challenge 1: PID Attribution
**Problem:** Async I/O (libaio, io_uring) may not have clear process context.

**Solution (Recommended):** Aggregate by app name
- When reading BPF map, translate each PID → app name
- Same approach as CPU tracking
- App name persists even if underlying process details are fuzzy

### Challenge 2: Read vs. Write Distinction
**Problem:** Tracepoint field names vary by kernel version.

**Solution:**
- Use stable fields: `args->op` (REQ_OP_READ=0, REQ_OP_WRITE=1)
- Fallback: check `args->rwbs` string prefix ('R' or 'W')
- Test on multiple kernel versions during Phase 1

### Challenge 3: Virtual Filesystems & Containers
**Problem:** Container I/O appears to be from the container's PID namespace, not the host.

**Solution:**
- Read `/proc/[pid]/cgroup` or namespace info to identify container
- Track separately if needed, or aggregate at container level
- This is a future enhancement; Phase 1 tracks host processes

### Challenge 4: Buffered I/O
**Problem:** Page cache means not all filesystem operations trigger block I/O immediately.

**Solution:**
- Block tracepoints capture actual device I/O, not all filesystem syscalls
- This is *accurate* — it's what actually hit the disk
- If you need filesystem-level tracking, use a separate syscall tracer (future work)

---

## Comparison: Disk I/O Methods

| Method | Accuracy | Overhead | Scope |
|--------|----------|----------|-------|
| eBPF block tracepoints | High | <0.5% | Actual device I/O |
| `/proc/[pid]/io` | Medium | Low | Per-process approximation |
| `iotop` (eBPF-based) | High | ~1% | Interactive tool (reference) |
| `iostat` | System-wide | <0.1% | Aggregate, no per-process |

**Recommendation**: eBPF provides the best balance of accuracy and low overhead.

---

## References

- Linux block device tracepoints: https://www.kernel.org/doc/html/latest/trace/events-kmem.html
- Block I/O documentation: https://www.kernel.org/doc/html/latest/block/
- libbpf tracepoint examples: https://github.com/libbpf/libbpf-bootstrap/tree/master/examples
- iotop source (eBPF reference): https://github.com/iovisor/iotop
- Linux sector size: typically 512 bytes (4K on newer drives, queried via `/sys/block/*/queue/hw_sector_size`)

---

## Next Steps

1. Review this plan with the team
2. Prototype eBPF program on Linux 5.x+ kernel
3. Test with synthetic I/O workloads (fio, dd)
4. Implement Phase 1 and Phase 2
5. Benchmark overhead against target (<1% CPU for heavy I/O workloads)

---

*Document created as part of heimwatch CPU, Disk, and Network tracking architecture planning.*
