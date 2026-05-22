//! Shared metric types for heimwatch.
//!
//! All metric types and records are defined here so that collectors, storage,
//! web, and TUI can all use them without creating circular dependencies.

use serde::{Deserialize, Serialize};

/// Enum of all metric types tracked by heimwatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MetricType {
    Net,
    Pwr,
    Foc,
    Cpu,
    Mem,
    Dsk,
    Gpu,
    GpuProc,
}

/// All metric types in order. Used for iterating across all metric types.
/// If a new metric type is added to the enum, add it here as well.
pub const ALL_METRIC_TYPES: &[MetricType] = &[
    MetricType::Net,
    MetricType::Pwr,
    MetricType::Foc,
    MetricType::Cpu,
    MetricType::Mem,
    MetricType::Dsk,
    MetricType::Gpu,
    MetricType::GpuProc,
];

impl MetricType {
    /// Returns the 3-character prefix used as the key prefix in sled.
    pub fn prefix(&self) -> &'static str {
        match self {
            MetricType::Net => "net",
            MetricType::Pwr => "pwr",
            MetricType::Foc => "foc",
            MetricType::Cpu => "cpu",
            MetricType::Mem => "mem",
            MetricType::Dsk => "dsk",
            MetricType::Gpu => "gpu",
            MetricType::GpuProc => "gpc",
        }
    }
}

/// Network traffic data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkData {
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub connections: u32,
}

/// Power consumption data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerData {
    pub watt_usage: f32,              // RAPL package watts if available, else 0.0
    pub battery_percent: Option<f32>, // 0–100, None if no battery present
    pub charging: bool,               // any AC online OR battery status == "Charging"

    // Optional fields for future use and breakdown
    pub rapl_package_watts: Option<f32>, // measured CPU package power from RAPL
    pub rapl_core_watts: Option<f32>,    // optional: CPU core domain (PP0) from RAPL
    pub battery_current_ua: Option<i64>, // µA, negative = discharging
    pub battery_voltage_uv: Option<u64>, // µV
    pub avg_cpu_freq_ratio: Option<f32>, // average(cur_freq/max_freq) across all cpufreq policies
    pub display_brightness: Option<f32>, // average(cur/max) across all backlight devices, 0.0–1.0
    pub is_wifi: Option<bool>, // true if default route is over WiFi, false if Ethernet, None if unknown
}

/// Window focus data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FocusData {
    pub app_id: String,
    pub duration_ms: u64,
}

/// CPU time and usage data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuData {
    /// Nanoseconds of CPU time consumed in the measurement interval.
    pub cpu_time_ns: u64,
    /// Percentage of total system CPU capacity used (0-100 × num_cores).
    /// Calculated as (cpu_time_ns / (interval_ns × num_cores)) × 100.
    pub cpu_usage_percent: f32,
}

/// Memory usage data (aggregated across all processes for an app).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryData {
    pub rss_bytes: u64,
    pub vms_bytes: u64,
    pub swap_bytes: u64,
    pub process_count: u32,
}

/// Disk I/O data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskData {
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub mount_point: String,
}

/// GPU vendor identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Unknown,
}

/// GPU usage data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuData {
    pub gpu_index: u32,
    pub vendor: GpuVendor,
    pub name: String,
    pub usage_percent: Option<f32>,
    pub vram_used_bytes: Option<u64>,
    pub vram_total_bytes: Option<u64>,
    pub temperature_celsius: Option<f32>,
    pub power_draw_watts: Option<f32>,
    pub core_clock_mhz: Option<u32>,
    pub memory_clock_mhz: Option<u32>,
}

/// Per-process GPU usage, collected via /proc/<pid>/fdinfo DRM scanning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuProcessData {
    pub gpu_index: u32,
    /// GFX engine utilization % over the poll interval (None if no engine data available).
    pub usage_percent: Option<f32>,
    /// VRAM held by this process across all its DRM fds (bytes).
    pub vram_used_bytes: Option<u64>,
}

/// Tagged union of all metric payload types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum MetricPayload {
    #[serde(rename = "net")]
    Net(NetworkData),
    #[serde(rename = "pwr")]
    Pwr(PowerData),
    #[serde(rename = "foc")]
    Foc(FocusData),
    #[serde(rename = "cpu")]
    Cpu(CpuData),
    #[serde(rename = "mem")]
    Mem(MemoryData),
    #[serde(rename = "dsk")]
    Dsk(DiskData),
    #[serde(rename = "gpu")]
    Gpu(GpuData),
    #[serde(rename = "gpu_proc")]
    GpuProc(GpuProcessData),
}

impl MetricPayload {
    /// Returns the MetricType variant for this payload.
    pub fn metric_type(&self) -> MetricType {
        match self {
            MetricPayload::Net(_) => MetricType::Net,
            MetricPayload::Pwr(_) => MetricType::Pwr,
            MetricPayload::Foc(_) => MetricType::Foc,
            MetricPayload::Cpu(_) => MetricType::Cpu,
            MetricPayload::Mem(_) => MetricType::Mem,
            MetricPayload::Dsk(_) => MetricType::Dsk,
            MetricPayload::Gpu(_) => MetricType::Gpu,
            MetricPayload::GpuProc(_) => MetricType::GpuProc,
        }
    }
}

/// A single metric record — the unit of data stored and retrieved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricRecord {
    /// Application or process name (no PID — storage is app-name indexed).
    pub app_name: String,
    /// Unix epoch seconds.
    pub timestamp: u64,
    /// The metric payload.
    pub payload: MetricPayload,
}

/// Aggregated network statistics for an app across a time range.
#[derive(Debug, Clone)]
pub struct AppNetworkStats {
    pub app_name: String,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
}

/// Aggregated focus time statistics for an app across a time range.
#[derive(Debug, Clone)]
pub struct AppFocusStats {
    pub app_name: String,
    pub total_duration_ms: u64,
}

/// Per-app power consumption statistics aggregated across a time range.
///
/// The `on_battery` field indicates whether the query was filtered to a specific power state.
/// When `on_battery=true`, only metrics from periods when the system was discharging were included.
/// When `on_battery=false`, the query included both on-battery and plugged-in periods (no filter).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppPowerStats {
    pub app_name: String,
    pub power_pct: f32,            // 0–100, share of system power
    pub power_score: f32,          // unnormalized composite score
    pub on_battery: bool, // true if query was filtered to on-battery state; false if unfiltered or plugged-in
    pub cpu_contribution: f32, // 0–1, fractional contribution from CPU
    pub gpu_contribution: f32, // 0–1
    pub display_contribution: f32, // 0–1, fractional contribution from screen focus time
    pub disk_contribution: f32, // 0–1
    pub net_contribution: f32, // 0–1
    pub mem_contribution: f32, // 0–1
}
