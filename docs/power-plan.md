# Power Usage Attribution Plan for Heimwatch

## Overview

This document describes the design for calculating per-app relative power consumption in Heimwatch. The goal is to answer "which apps use the most power?" and "does this app drain my battery?" — not to report absolute watts, but to show each app's percentage share of total system power.

Currently, Heimwatch collects CPU, memory, disk I/O, network, GPU, and screen-time metrics per app. These can be combined into a single "power score" that ranks apps by power consumption.

**Key principles:**
- Privacy-first: all data stays local, no cloud sync or telemetry
- Relative rankings, not absolute calibration: the percentages are meaningful within a session, not against standard workloads
- Battery-aware: tag measurements with AC vs. battery state so users see "this app was active while on battery"
- Extensible: the weighting system allows future refinement with additional sensors (RAPL, display brightness, CPU frequency scaling)

---

## 1. Power Source Detection

### Implementation: `PowerCollector`

A new collector reads battery and AC adapter state from Linux sysfs every 30 seconds.

#### Discovery

On startup, enumerate `/sys/class/power_supply/` and read the `type` file in each subdirectory to classify nodes:
- `type = "Battery"` → battery node (usually `BAT0`, `BAT1`, etc.)
- `type = "Mains"` or `"AC"` → AC adapter node (usually `AC0`, `ACAD`, `Mains`, etc.)

This approach is portable across different hardware: Intel systems use `ACAD`, some AMD systems use `Mains`, and laptop models vary in their battery numbering.

#### State Reading

**Battery state** (from the first battery node found):
```
capacity_path = /sys/class/power_supply/BAT*/capacity        # integer 0–100
status_path   = /sys/class/power_supply/BAT*/status          # "Charging", "Discharging", "Not charging", "Full"
current_path  = /sys/class/power_supply/BAT*/current_now     # µA (negative = discharging)
voltage_path  = /sys/class/power_supply/BAT*/voltage_now     # µV
```

**AC state** (from any mains node):
```
online_path = /sys/class/power_supply/{AC0,ACAD,Mains,etc}/online  # 0 or 1
```

#### Derived Power State

From the above readings:
- `charging: bool` = any AC node has `online == 1` OR any battery has `status == "Charging"`
- `battery_percent: Option<f32>` = capacity from the first battery node, or `None` if no battery present
- `battery_current_ua: Option<i64>` = signed current (negative when discharging)
- `battery_voltage_uv: Option<u64>` = battery voltage in microvolts

#### Optional: RAPL (Actual CPU Package Power)

If available, read CPU package energy from Intel RAPL:

```
rapl_path = /sys/class/powercap/intel-rapl/intel-rapl:0/energy_uj
max_range = /sys/class/powercap/intel-rapl/intel-rapl:0/max_energy_range_uj
```

Store two consecutive readings (30 seconds apart) and compute power:
```
rapl_package_watts = (energy_uj_now - energy_uj_prev) / 1_000_000 / 30
```

This is the measured CPU package power. Watch for counter wraparound (max value is in max_range_uj — typically ~262 kJ, which wraps in ~30 minutes if power is high, but 30s readings are safe).

Gracefully degrade: if RAPL is unavailable (no `/sys/class/powercap/` directory, permission denied, or hardware doesn't support it), use fixed weights instead (see section 3).

#### Storage

Emit one `MetricRecord` per poll (30s interval):
```rust
MetricRecord {
    app_name: "system".to_string(),
    timestamp: now_unix_seconds,
    payload: MetricPayload::Pwr(PowerData {
        watt_usage: rapl_package_watts_or_zero,
        battery_percent: Some(capacity),
        charging,
        rapl_package_watts: Some(rapl_package_watts_or_zero),
        rapl_core_watts: None,  // optional: read intel-rapl:0:0
        battery_current_ua: Some(current),
        battery_voltage_uv: Some(voltage),
    }),
}
```

The sled key is `pwr:{timestamp:020}:system`, following the existing schema.

#### Implementation Location

```
crates/heimwatch-collector/src/power/
    mod.rs       — pub use + platform dispatch
    linux.rs     — PowerCollector::new(), collect_power(), run()
    stub.rs      — returns Err("Power collection not available on this platform")
```

Integrate into `PlatformCollector` (crates/heimwatch-collector/src/lib.rs) following the pattern used by `MemoryCollector`:

```rust
pub struct PlatformCollector {
    // ... existing fields ...
    power: Option<PowerCollector>,
}

impl PlatformCollector::new() {
    let power = match PowerCollector::new() {
        Ok(pc) => Some(pc),
        Err(e) => {
            if e.to_string().contains("not yet implemented") {
                log::debug!("Power collection not available: {}", e);
            } else {
                log::warn!("Power collector initialization failed: {}", e);
            }
            None
        }
    };
    // ...
}

pub fn take_power_collector(mut self) -> Option<PowerCollector> {
    self.power.take()
}
```

Spawn the collector task in the daemon (crates/heimwatch-daemon/src/lib.rs):

```rust
if let Some(pc) = collector.take_power_collector() {
    log::info!("Power tracking active");
    let tx = event_tx.clone();
    let shutdown = shutdown_tx.subscribe();
    tokio::spawn(async move {
        if let Err(e) = pc.run(tx, shutdown).await {
            log::error!("Power tracking error: {}", e);
        }
    });
}
```

---

## 2. Battery State Tagging Strategy

### Problem

We need to tag each metric with "was this measured while on battery or while plugged in?" so users can filter queries to answer "what drained my battery?" independently of "what uses the most power overall?"

### Solution: Separate `Pwr` Records (No Schema Changes)

The `PowerCollector` emits `MetricPayload::Pwr` records every 30 seconds. All other collectors (CPU, memory, disk, network, GPU) emit records every 5–10 seconds.

At query time, for any given metric record at timestamp `ts`, find the most recent `Pwr` record at or before `ts` to determine the power state:

```
pwr_record = get_power_state_at(ts)  // find most recent Pwr at or before ts
on_battery = pwr_record.charging == false && pwr_record.battery_percent.is_some()
```

This approach:
- **No schema migration:** `MetricRecord` stays unchanged
- **No per-collector coupling:** collectors don't need to know about power state
- **Efficient:** the join is a single reverse range scan per query, not per record
- **Flexible:** the power state can be queried independently (e.g., show battery level history over time)

Trade-off: Small inaccuracy if power state changed mid-interval (e.g., plugged in 3 seconds before a 30-second poll). This granularity is fine for hour/day-level views.

### Implementation

Add to `StorageLayer` (crates/heimwatch-storage/src/db.rs):

```rust
/// Get the most recent Pwr record at or before the given timestamp.
pub fn get_power_state_at(&self, ts: u64) -> Result<Option<PowerData>> {
    // Range scan on "pwr:*" in reverse, return the first match
}
```

This uses sled's range scan: `tree.range(range_start(Pwr, 0)..=range_end(Pwr, ts)).rev().next()`.

---

## 3. Per-App Power Attribution

### Three Candidate Formulas

The goal is to combine CPU%, GPU%, display (via focus time), disk I/O, network, and memory into a single "power score" per app, then normalize across all apps to get percentages.

#### Approach A: Fixed-Weight Linear Combination (Recommended Baseline)

Assign each resource a fixed weight based on typical laptop power breakdown:

```
power_score(app) =
    0.40 × cpu_usage_percent
  + 0.20 × gpu_usage_percent         (from GpuProcessData, if available; else 0)
  + 0.15 × focus_time_fraction       (time app had screen focus; 0–100)
  + 0.10 × disk_io_normalized        (log2-scaled read_bytes + write_bytes)
  + 0.10 × net_io_normalized         (log2-scaled tx_bytes + rx_bytes)
  + 0.05 × mem_rss_fraction          (app_rss_bytes / total_rss_bytes)
```

**Normalization:**
1. For all apps in the query window, compute the above score.
2. Normalize non-percentage metrics to [0, 100]:
   - `focus_time_fraction = Σ(FocusData.duration_ms for app) / (end - start) × 1000 × 100` (fetch from `foc:{timestamp}:{app_name}` records)
   - `disk_io_normalized = log2(1 + total_bytes) / log2(1 + max_bytes) × 100`
   - `net_io_normalized = log2(1 + total_bytes) / log2(1 + max_bytes) × 100`
   - `mem_rss_fraction = (app_rss / total_system_rss) × 100`
3. Normalize the score to a percentage:
   - `power_pct(app) = power_score(app) / Σ power_score(all apps) × 100`

**Weight rationale** (typical laptop breakdown):
- **CPU: 40%** — CPU clock scaling and voltage are the dominant power lever. A CPU-bound workload at 50% duty cycle consumes ~50% more power than idle. Frequency doubling increases power consumption by ~2–2.5× (quadratic relationship to voltage). Reduced from 50% to accommodate display attribution.
- **GPU: 20%** — Discrete GPU and integrated GPU can consume 20–35% of system power when under load. Per-process GPU usage is available from fdinfo. Reduced from 25% to accommodate display attribution.
- **Display: 15%** — Screen backlight is often the largest single power consumer on laptops (5–15W). Apps with screen focus drive the display. The `focus_time_fraction` directly attributes display power to whichever app had user attention. **Limitation (Phase 1):** treats all focused time as equivalent (full-brightness assumption). Phase 4 enhancement: refine with `display_brightness_fraction` from `/sys/class/backlight/*/brightness`, scaling as `focus_time_fraction × display_brightness_fraction / 100`.
- **Disk I/O: 10%** — SSDs consume 3–6W during intensive write workloads; idle ~0.1W. An app doing heavy I/O is penalized but not dominantly.
- **Network: 10%** — WiFi radio draws 1–3W when active; Ethernet is much lower. This is a rough proxy since we don't track WiFi vs. Ethernet separately yet.
- **Memory: 5%** — DRAM refresh is nearly constant (~5W for 16GB); the app's share is proportional to its RSS. This is the weakest signal but ensures all resources are accounted for.

**Advantages:**
- Simple, deterministic, no kernel dependencies
- Works on any system without special instrumentation
- Provides stable baseline for dashboard/TUI display

**Limitations:**
- Weights are educated guesses, not calibrated to this specific hardware
- A CPU-bound app at 50% usage is assumed to use 50% of CPU power, but this depends heavily on workload (integer math vs. floating-point vs. branches); an efficient ARM CPU and a power-hungry Intel desktop diverge significantly for the same `cpu_usage_percent`
- No correlation to actual watts — useful for relative rankings but not for power budgeting

---

#### Approach B: RAPL-Calibrated CPU Weight (Enhanced Accuracy)

If RAPL is available, use the measured CPU package power instead of the guessed weight:

```
rapl_package_watts = measured from /sys/class/powercap/intel-rapl/intel-rapl:0/energy_uj deltas
total_cpu_pct = Σ cpu_usage_percent (all apps in window)

power_score(app) =
    rapl_package_watts × (app_cpu_pct / total_cpu_pct)    // actual CPU watts
  + 0.20 × gpu_usage_percent
  + 0.15 × focus_time_fraction
  + 0.10 × disk_io_normalized
  + 0.10 × net_io_normalized
  + 0.05 × mem_rss_fraction
```

Then normalize the composite score to percentages (same as Approach A).

Store `rapl_package_watts` in `PowerData.watt_usage` for display on the dashboard (e.g., "System is drawing 25W from CPU right now").

**Advantages:**
- CPU attribution is physically grounded: if RAPL says the CPU package is drawing 15W and Firefox uses 30% of CPU time, Firefox is attributed 4.5W of CPU power (proportionally accurate).
- Works on AMD systems as well (AMD RAPL exposes package domain).
- Single binary works across kernel versions (RAPL counters are stable).
- Opens the door to future energy budgeting ("this app will drain the battery in ~2 hours at current rate").

**Limitations:**
- Requires kernel support: `CONFIG_INTEL_RAPL_CORE=y` (Intel) or equivalent (AMD). Not all systems have RAPL.
- RAPL is a package-level measurement: if the CPU package is drawing 15W, we can't distinguish which cores/threads contributed most. We fall back to proportional allocation by `cpu_usage_percent`.
- Counter wraparound is rare but possible (max_energy_range_uj is ~262 kJ — 30-minute window at sustained 145W). Gracefully degrade to Approach A if wraparound is detected.
- GPU/display/disk/net weights are still guesses; only CPU is grounded.
- Display attribution via `focus_time_fraction` treats all focused time as equivalent (assumes full brightness). Refine with display brightness data in Phase 4.

**Recommendation:** Implement RAPL as an **enhancement** to Approach A. Check if `/sys/class/powercap/intel-rapl/` exists on startup; if so, read RAPL and use Approach B. Otherwise fall back to fixed weights.

---

#### Approach C: Battery Drain Rate Correlation (On-Battery Only)

While on battery, the instantaneous battery power draw can be measured:

```
battery_power_w = |current_now_µA| × voltage_now_µV / 1_000_000_000_000
```

Attribute this power proportionally to apps using the same resource ratios as Approach A.

**Advantages:**
- Total system power including all consumers: display backlight, WiFi radio, keyboard backlight, CPU, etc.
- Directly answers "how fast is my battery draining right now?"

**Limitations:**
- Works only while discharging (no data while plugged in).
- Battery current sensors are noisy and require 10–30 second smoothing.
- Cannot isolate which resources are consuming the power: a 20W draw could be 10W CPU + 5W GPU + 3W display + 2W WiFi, but we don't know without RAPL.
- Sensor paths vary across BMCs (`current_now` may be in mA vs. µA).

**Recommendation:** Use as a **secondary indicator** only. Show "system is drawing 20W from battery right now" alongside per-app scores, but don't use it as the primary attribution method. Require RAPL or fixed weights for per-app breakdown.

---

### Summary: Recommended Approach

**Start with Approach A (fixed weights).** It's simple, portable, and provides stable rankings.

**Add Approach B (RAPL) as an enhancement.** On systems with RAPL available (most Intel/AMD laptops built in the last 5 years), the CPU attribution is more accurate. The fallback to fixed weights is automatic if RAPL is unavailable.

**Show Approach C (battery drain) as a system-level metric** ("System power draw: 18W from battery"), not as a per-app attribution.

---

## 4. Data Model Changes

### Expand `PowerData`

In `crates/heimwatch-core/src/metrics.rs`, extend `PowerData`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerData {
    pub watt_usage: f32,                 // RAPL package watts if available, else 0.0
    pub battery_percent: Option<f32>,    // 0–100, None if no battery present
    pub charging: bool,                  // any AC online OR battery status == "Charging"
    
    // New optional fields (for future use and breakdown):
    pub rapl_package_watts: Option<f32>, // measured CPU package power from RAPL
    pub rapl_core_watts: Option<f32>,    // optional: CPU core domain (PP0) from RAPL
    pub battery_current_ua: Option<i64>, // µA, negative = discharging
    pub battery_voltage_uv: Option<u64>, // µV
}
```

All new fields are `Option` to maintain backward compatibility with existing serialized records.

### Add `AppPowerStats`

In `crates/heimwatch-core/src/metrics.rs` (or a new `power.rs` sub-module), define the output type for power queries:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppPowerStats {
    pub app_name: String,
    pub power_pct: f32,                 // 0–100, share of system power
    pub power_score: f32,               // unnormalized composite score
    pub on_battery: bool,               // from nearest Pwr record
    pub cpu_contribution: f32,          // 0–1, fractional contribution from CPU
    pub gpu_contribution: f32,          // 0–1
    pub display_contribution: f32,      // 0–1, fractional contribution from screen focus time
    pub disk_contribution: f32,         // 0–1
    pub net_contribution: f32,          // 0–1
    pub mem_contribution: f32,          // 0–1
}
```

These are computed at query time, not stored in the database. The contribution fields help users understand the breakdown ("Netflix is 85% display, 10% GPU, 5% CPU"; "Blender is 70% GPU, 25% CPU, 5% disk I/O").

---

## 5. Storage Query API

Add the following methods to `StorageLayer` (crates/heimwatch-storage/src/db.rs):

### `get_power_state_at(timestamp: u64) -> Result<Option<PowerData>>`

Returns the most recent `Pwr` record at or before the given timestamp. Used to determine battery state for other metric records.

Implementation: `tree.range(range_start(Pwr, 0)..=range_end(Pwr, timestamp)).rev().next()`

### `get_power_stats(start: u64, end: u64, on_battery: Option<bool>) -> Result<Vec<AppPowerStats>>`

The primary power query. Steps:

1. Fetch all `Cpu`, `Mem`, `Dsk`, `Net`, `GpuProc`, `Foc`, and `Pwr` records in `[start, end]`.
2. If `on_battery = Some(true)`, filter to time windows where power state was on battery (read from `Pwr` records).
3. For each app, aggregate metrics across the window:
   - `cpu_pct_avg` = mean of `CpuData.cpu_usage_percent`
   - `gpu_pct_max` = max of `GpuProcessData.usage_percent` (or 0 if no GPU data)
   - `focus_time_ms_total` = sum of `FocusData.duration_ms` (time app had screen focus)
   - `disk_bytes_total` = sum of `DiskData.read_bytes + write_bytes`
   - `net_bytes_total` = sum of `NetworkData.tx_bytes + rx_bytes`
   - `mem_rss_avg` = mean of `MemoryData.rss_bytes`
4. Normalize non-percentage metrics to [0, 100]:
   - `focus_fraction = (focus_time_ms_total / ((end - start) × 1000)) × 100` (percentage of query window with focus)
   - Find `max_disk_bytes` and `max_net_bytes` across all apps
   - `disk_normalized = log2(1 + disk_bytes) / log2(1 + max_disk_bytes) × 100`
   - `net_normalized = log2(1 + net_bytes) / log2(1 + max_net_bytes) × 100`
   - `mem_fraction = (mem_rss_avg / total_system_rss_avg) × 100`
5. Compute power score:
   ```
   if rapl_package_watts available:
       cpu_score = rapl_package_watts × (app_cpu_pct / Σ cpu_pct_all_apps)
       weights = [cpu_score, 0.20, 0.15, 0.10, 0.10, 0.05]
   else:
       weights = [0.40, 0.20, 0.15, 0.10, 0.10, 0.05]
   
   power_score(app) = dot(weights, [cpu, gpu, focus, disk, net, mem])
   ```
6. Normalize all scores to percentages: `power_pct = score / Σ score(all apps) × 100`
7. Compute contribution fractions: `cpu_contrib = (cpu_score / power_score)`, `display_contrib = (focus_score / power_score)`, etc. for that app
8. Return sorted descending by `power_pct`.

### `get_top_apps_by_power(start: u64, end: u64, on_battery: Option<bool>, limit: usize) -> Result<Vec<AppPowerStats>>`

Delegates to `get_power_stats` and truncates to the top `limit` apps. Parallel to `get_top_apps_by_network`.

### `get_system_power_history(start: u64, end: u64) -> Result<Vec<MetricRecord>>`

Returns all `MetricType::Pwr` records in the time range, for plotting battery level and RAPL power over time on the dashboard.

---

## 6. Display and Reporting

### CLI Snapshot

Add a `snapshot power` subcommand to the daemon:

```bash
heimwatch-daemon snapshot power --window 3600
```

Output (example):
```
Power Usage (last 3600s)

Top Apps by Power Percentage (Plugged In):
  1. Firefox         35.2%  (CPU: 25%, GPU: 0%, Disk: 8%, Network: 2%, Memory: 0%)
  2. VS Code         22.1%  (CPU: 18%, GPU: 0%, Disk: 3%, Network: 1%, Memory: 0%)
  3. Chrome          18.5%  (CPU: 15%, GPU: 2%, Disk: 1%, Network: 0%, Memory: 0%)

Top Apps by Power Percentage (On Battery):
  (no data: not on battery during this window)

System Power (latest):
  Battery: 75%
  Charging: false
  RAPL Package: 18.2W
```

### Web Dashboard

Add to the future dashboard:
- **Bar chart:** Top 10 apps by power percentage, stacked by contribution (CPU/GPU/disk/net/memory colors)
- **Toggle:** "Show while on battery only" / "Show while plugged in only"
- **Line chart:** Battery level over time (from `PowerData.battery_percent`)
- **Line chart:** RAPL package power over time (from `PowerData.watt_usage`)
- **Card:** Current system power draw ("18.2W from CPU package")

### TUI

Add a new column to the app table:
- **`Power%`** — current power percentage (requires a short query window, e.g., last 60 seconds)
- Show in context of other metrics: `AppName | CPU% | Mem% | Network | Disk | Power%`

---

## 7. Additional Data Sources (Future Enhancements)

Ranked by impact on accuracy:

### 1. RAPL (Highest Impact) ✓ Implemented in Phase 1

See Approach B above. `/sys/class/powercap/intel-rapl/` gives measured CPU package power.

### 2. CPU Frequency Scaling

A CPU core running at 4.0 GHz with 50% usage consumes more power than a core at 1.0 GHz at 50% usage. Frequency scales roughly quadratically with voltage (power ∝ f × V²):

```
freq_adjusted_cpu_score = cpu_usage_pct × (cur_freq / max_freq)²
```

Read `cur_freq` from `/sys/devices/system/cpu/cpufreq/policy{N}/scaling_cur_freq` (in kHz) and `max_freq` from `/sys/devices/system/cpu/cpufreq/policy{N}/cpuinfo_max_freq`.

Poll once per CPU collection interval (5s). Average across the query window.

### 3. Display Brightness

Screen backlight is often the largest single consumer (5–15W on laptops). Power is roughly proportional to brightness.

```
display_power_fraction = brightness / max_brightness
```

Add `display_brightness_fraction: Option<f32>` to `PowerData`. Store as a separate `MetricType` (e.g., `Sys` for system-level components) and report "Unattributed Power: 25% (likely display, WiFi radio, other hardware)".

Read from `/sys/class/backlight/*/brightness` and `/sys/class/backlight/*/max_brightness`.

### 4. WiFi vs. Ethernet

WiFi draws 1–3W additional power compared to Ethernet. Increase the `W_net` weight for WiFi traffic:

```
if network_interface_is_wifi:
    weight_net = 0.15   // vs 0.05 for Ethernet
else:
    weight_net = 0.05
```

Detect interface type from `/sys/class/net/{iface}/type` (801 = WiFi, 1 = Ethernet).

### 5. Thermal Throttling

When CPU temperature exceeds a threshold, the kernel throttles frequency below the nominal max. This distorts CPU% readings: an app that "looked 30% active" was actually throttled and consuming less power.

Store temperature from `/sys/class/hwmon/hwmon*/temp*_input` and add a `thermal_throttling_active: bool` flag to indicate when readings are distorted.

### 6. NVMe vs. SSD Power

High-throughput write workloads on NVMe can consume 3–6W. Distinguish from slower SATA SSDs by checking `/sys/block/{dev}/queue/rotational` (0 = SSD) and adjusting disk I/O weight accordingly.

---

## 8. Implementation Sequencing

Phases are organized by dependency; earlier phases enable later ones.

**Phase 1: Core Infrastructure**
1. Implement `PowerCollector` (linux.rs + stub.rs) with battery state reading
2. Wire `PowerCollector` into `PlatformCollector` and daemon spawn loop
3. Expand `PowerData` struct (add RAPL and battery current fields, backward-compatible)
4. Implement `StorageLayer::get_power_state_at()`

**Phase 2: Power Attribution**
5. Implement `StorageLayer::get_power_stats()` and `get_top_apps_by_power()` with Approach A (fixed weights)
6. Add `AppPowerStats` type to `heimwatch-core`
7. Implement `StorageLayer::get_system_power_history()`

**Phase 3: User-Facing Reporting**
8. Add `snapshot power` subcommand to the daemon CLI

**Phase 4: Enhancements (Optional)**
9. Add RAPL integration to Approach B weighting in `StorageLayer`
10. CPU frequency scaling factor in power score
11. Display brightness tracking as system-level unattributed power component
12. WiFi vs. Ethernet network weight adjustment

**Phase 5: Dashboard/TUI (Post-Core)**
13. Add power percentage bar chart to dashboard
14. Add Power% column to TUI
15. Add battery level line chart

---

## 8b. Implementation Notes and Phase 2+ Optimizations

### Query Performance: `get_power_state_at()`

**Current (Phase 1):** Range scan from timestamp 0 to the query timestamp.

**Phase 2 optimization:** Since `Pwr` records are emitted every 30 seconds and queries need only the most recent record before a given timestamp, optimize the scan to start from `timestamp - (30 * 60)` (last 30 poll cycles, ~15 minutes) instead of 0. This reduces scans from millions of records to at most 30 for year-long datasets.

```rust
let recent_window = timestamp.saturating_sub(30 * 60); // Look back 30 minutes
let range_start = crate::keys::range_start(&MetricType::Pwr, recent_window);
let range_end = crate::keys::range_end(&MetricType::Pwr, timestamp);
```

### RAPL Counter Wraparound Detection

**Current (Phase 1):** Uses `saturating_sub()` which silently clips to 0 on underflow (counter reset).

**Phase 2 enhancement:** Detect and log wraparound events (rare: ~every 30 min at sustained 145W on typical hardware). Max energy range is typically ~262 kJ from `/sys/class/powercap/intel-rapl/intel-rapl:0/max_energy_range_uj`.

```rust
let (delta_uj, wrapped) = if energy_uj >= *prev {
    (energy_uj - *prev, false)
} else {
    // Counter wrapped; compute delta assuming rollover
    let max_range = 262_000_000; // Read from max_energy_range_uj
    (max_range - *prev + energy_uj, true)
};
if wrapped {
    log::warn!("RAPL energy counter wraparound detected; reading may be inaccurate");
}
```

### Multi-Socket and AMD RAPL Support

**Current (Phase 1):** Only checks `intel-rapl:0`.

**Phase 4 enhancement:** Iterate `/sys/class/powercap/` to find all available RAPL domains (intel-rapl:0, intel-rapl:1 on dual-socket systems; AMD systems use `amd_rapl`). Aggregate package power across all sockets.

---

## 9. Key sysfs Paths Reference

| What | Path | Unit |
|---|---|---|
| Battery capacity | `/sys/class/power_supply/BAT*/capacity` | 0–100 (%) |
| Battery status | `/sys/class/power_supply/BAT*/status` | String: "Charging", "Discharging", etc. |
| Battery current | `/sys/class/power_supply/BAT*/current_now` | µA (negative = discharging) |
| Battery voltage | `/sys/class/power_supply/BAT*/voltage_now` | µV |
| AC adapter online | `/sys/class/power_supply/{AC0,ACAD,Mains}/online` | 0 or 1 |
| Power supply type | `/sys/class/power_supply/*/type` | String: "Battery", "Mains", "USB", etc. |
| **RAPL package energy** | `/sys/class/powercap/intel-rapl/intel-rapl:0/energy_uj` | µJ (counter) |
| **RAPL core energy** | `/sys/class/powercap/intel-rapl/intel-rapl:0:0/energy_uj` | µJ (counter) |
| RAPL max energy range | `/sys/class/powercap/intel-rapl/intel-rapl:0/max_energy_range_uj` | µJ |
| CPU current frequency | `/sys/devices/system/cpu/cpufreq/policy{N}/scaling_cur_freq` | kHz |
| CPU max frequency | `/sys/devices/system/cpu/cpufreq/policy{N}/cpuinfo_max_freq` | kHz |
| Backlight brightness | `/sys/class/backlight/*/brightness` | integer |
| Backlight max | `/sys/class/backlight/*/max_brightness` | integer |
| Network interface type | `/sys/class/net/{iface}/type` | 1=Ethernet, 801=WiFi |
| CPU temperature | `/sys/class/hwmon/hwmon*/temp*_input` | millidegrees C |

---

## Summary

**Battery state:** `PowerCollector` reads `/sys/class/power_supply/` every 30s, stores `Pwr` records. Queries join by timestamp to tag other metrics with battery state.

**Attribution formula:** Approach A (fixed weights: CPU 50%, GPU 25%, disk 10%, network 10%, memory 5%). Approach B (RAPL-calibrated CPU weight) as an enhancement where RAPL is available.

**Data model:** Expand `PowerData` with optional RAPL and battery current fields. Add `AppPowerStats` as a query-time computed type.

**Query API:** Four new methods on `StorageLayer`: `get_power_state_at`, `get_power_stats`, `get_top_apps_by_power`, `get_system_power_history`.

**Reporting:** CLI `snapshot power` command shows top apps by percentage. Dashboard shows per-app breakdown and battery trends. TUI adds a `Power%` column.
