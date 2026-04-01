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
}

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
    pub watt_usage: f32,
    pub battery_percent: Option<f32>,
    pub charging: bool,
}

/// Window focus data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FocusData {
    pub window_title: String,
    pub duration_seconds: u32,
}

/// CPU usage data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuData {
    pub usage_percent: f32,
    pub core_count: u32,
}

/// Memory usage data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryData {
    pub used_bytes: u64,
    pub total_bytes: u64,
}

/// Disk I/O data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskData {
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub mount_point: String,
}

/// GPU usage data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuData {
    pub usage_percent: f32,
    pub vram_used_bytes: u64,
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
