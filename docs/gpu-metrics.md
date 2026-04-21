# GPU Metrics Collection: Architecture & Decision

## Problem Statement

Heimwatch needs to track GPU resource usage (utilization, VRAM, temperature, power) alongside CPU, memory, and disk metrics. Unlike CPU (where `sched_switch` tracepoints give timing), GPU metrics present unique challenges:

1. **Multiple vendors on one machine**: laptops often have integrated GPU (Intel/AMD) + discrete GPU (NVIDIA/AMD); desktops can have multiple discrete GPUs
2. **Vendor-specific APIs**: each GPU vendor exposes metrics through different interfaces
3. **eBPF limitations**: kernel DRM tracepoints don't give utilization or hardware register reads

## Why eBPF Won't Work

Linux DRM subsystem has sparse tracepoints (`drm_vblank_event`, `drm_sched_job`, `drm_run_job`) that cover job submission/completion only. They do **not** provide:
- GPU utilization % (would need continuous sampling of GPU engine active cycles)
- VRAM occupancy (requires GPU memory management queries)
- Temperature, power draw, clock speeds (hardware register reads, not scheduling events)

The DRM tracepoints lack kernel support across vendors — NVIDIA's proprietary driver exposes zero DRM tracepoints; AMD's amdgpu driver has scheduler tracepoints but no utilization aggregation; Intel's i915/xe drivers have fence/request tracepoints but no GPU time accumulation like `sched_switch`.

**Conclusion**: eBPF can track GPU job counts (future optimization) but cannot be the primary collection mechanism for system-level GPU metrics. Use **sysfs polling + vendor libraries** instead.

---

## Architecture: Per-Vendor Backends

### Data Flow

```
Linux GPU Detection
  ↓
Enumerate /sys/class/drm/card{0,1,2,...}
  ↓
For each card: read vendor ID, resolve driver
  ↓
Instantiate appropriate backend:
  - AMD (vendor 0x1002) → AmdBackend (sysfs)
  - Intel (vendor 0x8086) → IntelBackend (sysfs)
  - NVIDIA (vendor 0x10de) → NvidiaBackend (NVML)
  - Unknown → GenericBackend (hwmon only)
  ↓
GpuCollector owns Vec<Box<dyn GpuBackend>>
  ↓
On each poll (5s): call backend.collect() → GpuData
  ↓
Emit MetricRecord with app_name = "gpu:N"
```

Each GPU backend is independent, so multi-GPU scenarios are trivial — just iterate the vec.

### Trait-Based Dispatch

```rust
pub trait GpuBackend: Send {
    fn collect(&mut self) -> anyhow::Result<GpuData>;
    fn gpu_name(&self) -> &str;
    fn gpu_index(&self) -> u32;
}
```

All backends are `Send` because they hold only file paths and optional library handles (no Rc/RefCell/locks). This allows GPU collection to run in a standard `tokio::spawn_blocking` task alongside other collectors.

---

## Per-Vendor Solutions

### AMD: Pure sysfs, Richest Interface

**Source**: `/sys/class/drm/cardN/device/` (amdgpu driver)

| Metric | File | Type | Notes |
|---|---|---|---|
| Utilization | `gpu_busy_percent` | 0-100% | Updated by driver; instant read |
| VRAM Used | `mem_info_vram_used` | bytes | Bytes |
| VRAM Total | `mem_info_vram_total` | bytes | Bytes |
| Core Clock | `pp_dpm_sclk` | MHz | Parse line with `*` marker for current state |
| Memory Clock | `pp_dpm_mclk` | MHz | Same as core clock |
| Temperature | hwmon `temp1_input` | millidegrees | Divide by 1000 |
| Power Draw | hwmon `power1_average` | microwatts | Divide by 1,000,000 for watts |
| Device Name | `product_name` or PCI subsystem | string | Fallback to PCI device name |

**Why AMD works without external library**: the amdgpu driver is the standard open-source driver. It exposes a comprehensive sysfs interface for all metrics. No library bindings required.

### Intel: sysfs (clocks, temp) + perf PMU (utilization - Phase 5)

**Source**: `/sys/class/drm/cardN/gt/gt0/` (i915/xe drivers)

| Metric | File | Type | Phase |
|---|---|---|---|
| Core Clock | `rps_cur_freq_mhz` | MHz | 1 |
| Max Clock | `rps_max_freq_mhz` | MHz | 1 |
| Min Clock | `rps_min_freq_mhz` | MHz | 1 |
| Temperature | hwmon `temp*_input` | millidegrees | 1 |
| Power Draw | hwmon `power*_average` | microwatts | 1 |
| Utilization | `/sys/bus/event_source/devices/i915/events/render/0/busy` (perf PMU) | busy ns delta | Phase 5 |
| VRAM Used | N/A | — | iGPU uses system RAM (tracked by MemoryCollector) |

**Phase 1 limitation**: i915/xe sysfs does not expose GPU utilization percentage directly. To get utilization, we need `perf_event_open()` on the i915 PMU render engine event (Phase 5). Until then, `usage_percent = None`.

**iGPU VRAM**: Intel iGPU is integrated and shares system RAM. There is no separate VRAM pool to track. System memory tracking (MemoryCollector) already covers this.

### NVIDIA: NVML Library (nvml-wrapper crate)

**Source**: NVML (NVIDIA Management Library), loaded at runtime via dlopen

| Metric | NVML Method | Notes |
|---|---|---|
| Utilization | `device.utilization_rates()` | Returns (gpu_util%, mem_util%) |
| VRAM Used | `device.memory_info()` | Returns (used, free, total) |
| VRAM Total | `device.memory_info()` | Same call |
| Temperature | `device.temperature(TemperatureSensor::Gpu)` | Celsius |
| Power Draw | `device.power_usage()` | Returns milliwatts; convert to watts |
| Core Clock | `device.clock(Clock::Graphics, ...)` | MHz |
| Memory Clock | `device.clock(Clock::Memory, ...)` | MHz |
| Device Name | `device.name()` | String |

**Runtime dlopen**: The `nvml-wrapper` crate supports loading `libnvidia-ml.so` at runtime. If the library is absent, backend initialization fails gracefully; detection skips NVIDIA cards and logs info (not error). The daemon still runs, just without NVIDIA metrics.

**Requires**: `CAP_SYS_ADMIN` or equivalent, same as other GPU management tools. The daemon process already needs this for eBPF probes.

### Generic Fallback: hwmon Only

If a GPU is detected but no vendor-specific backend applies (unknown vendor or stub driver), use `GenericBackend` which reads only hwmon:
- Temperature
- Power draw
- Everything else: `None`

This ensures graceful degradation for unknown GPUs.

---

## Multi-GPU Scenarios

### Detection Algorithm

```
1. Enumerate /sys/class/drm/ for card{0,1,2,...} entries (skip renderD*, controlD*)
2. For each cardN:
   a. Read device/vendor (e.g., 0x10de, 0x1002, 0x8086)
   b. Resolve device/driver/module symlink → driver name (nvidia, amdgpu, i915, xe, nouveau, ...)
   c. Match (vendor, driver) → select backend type
   d. Create backend with card_path, gpu_index=N
3. Return Vec<Box<dyn GpuBackend>> in detection order (card0, card1, ...)
```

### Laptop Example

```
/sys/class/drm/card0 → Intel vendor 0x8086, driver i915 → IntelBackend(0)
/sys/class/drm/card1 → NVIDIA vendor 0x10de, driver nvidia → NvidiaBackend(1)

GpuCollector.backends = [IntelBackend(0), NvidiaBackend(1)]

Poll 1: 
  - IntelBackend::collect() → GpuData{ gpu_index=0, name="Intel UHD 630", ... }
  - NvidiaBackend::collect() → GpuData{ gpu_index=1, name="NVIDIA RTX 4060", ... }
  - Emit MetricRecord app_name="gpu:0" and app_name="gpu:1"
```

### Desktop with Multiple Discrete GPUs

```
/sys/class/drm/card0 → AMD vendor 0x1002, driver amdgpu → AmdBackend(0)
/sys/class/drm/card1 → AMD vendor 0x1002, driver amdgpu → AmdBackend(1)

GpuCollector.backends = [AmdBackend(0), AmdBackend(1)]

Poll 1:
  - AmdBackend(0)::collect() → GpuData{ gpu_index=0, name="RX 7900 XTX", ... }
  - AmdBackend(1)::collect() → GpuData{ gpu_index=1, name="RX 7900 XT", ... }
```

All scenarios are handled by iterating the backends vec.

---

## Data Model

### GpuVendor Enum

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Unknown,
}
```

### GpuData Structure

```rust
pub struct GpuData {
    pub gpu_index: u32,                         // card number (0, 1, 2...)
    pub vendor: GpuVendor,                      // which vendor API was used
    pub name: String,                           // "NVIDIA RTX 4090", "Intel UHD 630", etc.
    pub usage_percent: Option<f32>,             // GPU utilization 0-100, None if not available
    pub vram_used_bytes: Option<u64>,           // VRAM currently in use, None for iGPU
    pub vram_total_bytes: Option<u64>,          // Total VRAM available, None for iGPU
    pub temperature_celsius: Option<f32>,       // GPU die temperature
    pub power_draw_watts: Option<f32>,          // Instantaneous power consumption
    pub core_clock_mhz: Option<u32>,            // Current GPU core frequency
    pub memory_clock_mhz: Option<u32>,          // Current memory frequency (None for iGPU)
}
```

All fields except `gpu_index`, `vendor`, and `name` are optional because not all vendors expose all metrics, and some may be unavailable (e.g., older kernels without sysfs support).

### MetricRecord Emission

Each GPU backend produces one `MetricRecord` per poll:

```rust
MetricRecord {
    app_name: format!("gpu:{}", gpu_index),    // e.g., "gpu:0", "gpu:1"
    timestamp: now_unix_seconds,
    payload: MetricPayload::Gpu(gpu_data),
}
```

GPU metrics are device-level, not process-level, so all metrics aggregate under `gpu:N`. Per-process GPU memory attribution (NVIDIA only) is a Phase 2 enhancement.

---

## File Structure

```
crates/heimwatch-collector/src/
  gpu/
    mod.rs                  # Platform dispatch (cfg linux/macos/windows)
    macos.rs                # Stub returning "not yet implemented"
    windows.rs              # Stub returning "not yet implemented"
    linux/
      mod.rs                # GpuCollector, run loop, GpuBackend trait
      detect.rs             # enumerate_gpus() → Vec<Box<dyn GpuBackend>>
      generic.rs            # Shared sysfs/hwmon helpers; GenericBackend
      amd.rs                # AmdBackend: amdgpu sysfs collection
      intel.rs              # IntelBackend: i915/xe sysfs (phase 5: + perf PMU)
      nvidia.rs             # NvidiaBackend: nvml-wrapper + graceful fallback
```

Pattern matches the existing `focus/linux/` structure, which splits Linux implementation into multiple files.

---

## Shared Utilities (generic.rs)

Helper functions used by all backends:

```rust
// Read single integer from sysfs file
fn read_sysfs_u64(path: &Path) -> Option<u64>

// Read string from sysfs file (trimmed)
fn read_sysfs_string(path: &Path) -> Option<String>

// Find hwmon path linked from a DRM device
fn find_hwmon_path(device_path: &Path) -> Option<PathBuf>

// Read temperature from hwmon (temp1_input in millidegrees)
fn read_hwmon_temp_celsius(hwmon: &Path) -> Option<f32>

// Read power from hwmon (power1_average in microwatts)
fn read_hwmon_power_watts(hwmon: &Path) -> Option<f32>

// GenericBackend struct: hwmon-only fallback
pub struct GenericBackend { ... }
impl GpuBackend for GenericBackend { ... }
```

These are the same kind of small utility functions already in `memory/linux.rs` and `cpu/linux.rs`.

---

## Integration Points

### crates/heimwatch-core/src/metrics.rs
- Add `GpuVendor` enum
- Expand `GpuData` with new optional fields
- Update `MetricPayload::Gpu(GpuData)` (no change needed)

### crates/heimwatch-collector/src/lib.rs
- Add `gpu: Option<GpuCollector>` to `PlatformCollector`
- Add `pub fn take_gpu_collector(&mut self) -> Option<GpuCollector>`
- Initialize in `PlatformCollector::new()` with error handling

### crates/heimwatch-collector/Cargo.toml
```toml
[features]
nvidia = ["dep:nvml-wrapper"]

[target.'cfg(target_os = "linux")'.dependencies]
nvml-wrapper = { version = "0.10", optional = true }
```

No new non-optional dependencies; AMD and Intel are pure sysfs.

### crates/heimwatch-daemon/src/
- Extract `take_gpu_collector()` in daemon's task setup
- Spawn into `tokio::spawn_blocking` with `run_collector_loop` (5s interval)
- Same pattern as CPU, disk, memory collectors

---

## Error Handling & Graceful Degradation

1. **No GPUs detected**: `GpuCollector::new()` returns `Ok(collector)` with empty backends vec → no GPU metrics emitted (clean)
2. **sysfs read failure**: backend logs debug/warn, returns `None` for affected field → other fields collected
3. **NVML library absent**: `NvidiaBackend::new()` returns error → detection skips NVIDIA card → logs info-level "NVML not available"
4. **GPU disappears** (Thunderbolt hot-unplug): backend's `collect()` returns error → collector removes that backend from vec → continues with remaining GPUs
5. **Unknown driver/vendor**: falls back to `GenericBackend` (hwmon only) → collects temperature and power only

In all cases: the daemon continues running. GPU collection does not block other metrics.

---

## Implementation Phases

**Phase 1: Core infrastructure** (no collection yet)
- Expand `GpuData` / add `GpuVendor` in `heimwatch-core`
- Create `gpu/` module structure, stubs, Linux detection
- Wire into `PlatformCollector` and daemon
- Verify code compiles; no runtime tests needed

**Phase 2: AMD backend** (sysfs, no external lib)
- Implement all metrics via amdgpu sysfs
- Test on AMD hardware or with mocked sysfs
- Highest value, fastest to implement

**Phase 3: Intel backend** (sysfs only)
- Implement temp, power, clocks
- Utilization returns `None` (Phase 5)
- Test on Intel iGPU hardware

**Phase 4: NVIDIA backend** (NVML)
- Feature-gated `nvidia` compilation
- Runtime dlopen graceful fallback
- Test with and without NVML library present

**Phase 5: Intel perf PMU**
- Add `perf_event_open` wrapper for i915/xe
- Implement GPU engine busy percentage
- Enable `usage_percent` for Intel

**Phase 6: Robustness**
- Hot-unplug detection and recovery
- Additional error handling and logging

---

## Key Design Decisions

| Decision | Rationale |
|---|---|
| No eBPF | DRM tracepoints don't provide utilization, VRAM, temp, power |
| sysfs + vendor APIs | AMD/Intel expose sysfs; NVIDIA requires NVML; covers all cases |
| Per-vendor backends | Simplifies multi-GPU; each backend handles its own discovery |
| `Vec<Box<dyn GpuBackend>>` | Handles 1, 2, or N GPUs without special cases |
| `app_name = "gpu:N"` | Device-level metrics, not process-level (future: per-process from NVML) |
| Optional NVML feature | Graceful degradation if NVIDIA not present |
| hwmon fallback | Unknown GPUs still report temp/power via generic interface |
| 5-second poll interval | GPU metrics don't need sub-second resolution for activity tracking |

---

## Future Enhancements

1. **Per-process GPU memory** (Phase 2+): NVML offers `device.processes()` → PID → VRAM; requires mapping PIDs to app_name
2. **GPU compute vs. graphics attribution**: require per-engine breakdown if available
3. **Encoder/decoder utilization**: AMD 6.x+ kernels expose `vcn_busy_percent` for video engines
4. **Thermal throttling detection**: warn if temperature near limit
5. **Power budgeting**: track power draw against TDP
6. **eBPF job counting** (optimization): count GPU jobs per PID for rough utilization ranking (research phase)

