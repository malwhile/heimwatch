use heimwatch_core::MetricType;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionConfig {
    #[serde(default = "default_retention_days")]
    pub cpu_days: u32,
    #[serde(default = "default_retention_days")]
    pub net_days: u32,
    #[serde(default = "default_retention_days")]
    pub pwr_days: u32,
    #[serde(default = "default_retention_days")]
    pub foc_days: u32,
    #[serde(default = "default_retention_days")]
    pub mem_days: u32,
    #[serde(default = "default_retention_days")]
    pub dsk_days: u32,
    #[serde(default = "default_retention_days")]
    pub gpu_days: u32,
    #[serde(default = "default_retention_days")]
    pub gpu_proc_days: u32,

    #[serde(default = "default_cleanup_interval")]
    pub cleanup_interval_hours: u64,
    #[serde(default)]
    pub export_before_delete: bool,
    #[serde(default)]
    pub export_dir: Option<String>,
}

fn default_retention_days() -> u32 {
    7
}

fn default_cleanup_interval() -> u64 {
    24
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            cpu_days: 7,
            net_days: 7,
            pwr_days: 7,
            foc_days: 7,
            mem_days: 7,
            dsk_days: 7,
            gpu_days: 7,
            gpu_proc_days: 7,
            cleanup_interval_hours: 24,
            export_before_delete: false,
            export_dir: None,
        }
    }
}

impl RetentionConfig {
    pub fn retention_days_for(&self, mt: MetricType) -> u32 {
        match mt {
            MetricType::Cpu => self.cpu_days,
            MetricType::Net => self.net_days,
            MetricType::Pwr => self.pwr_days,
            MetricType::Foc => self.foc_days,
            MetricType::Mem => self.mem_days,
            MetricType::Dsk => self.dsk_days,
            MetricType::Gpu => self.gpu_days,
            MetricType::GpuProc => self.gpu_proc_days,
        }
    }
}

#[derive(Debug)]
pub struct CleanupReport {
    pub deleted_count: u64,
    pub exported_count: u64,
    pub export_path: Option<PathBuf>,
}

#[derive(Debug)]
pub struct StorageStats {
    pub db_size_bytes: u64,
    pub record_counts: HashMap<MetricType, u64>,
    pub total_records: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TieredRetentionConfig {
    #[serde(default = "default_raw_hours")]
    pub raw_hours: u64,

    #[serde(default = "default_daily_keep_days")]
    pub daily_keep_days: u32,

    #[serde(default = "default_monthly_keep_months")]
    pub monthly_keep_months: u32,

    #[serde(default = "default_yearly_keep_years")]
    pub yearly_keep_years: u32,

    #[serde(default = "default_cleanup_interval")]
    pub cleanup_interval_hours: u64,

    #[serde(default)]
    pub export_before_delete: bool,

    #[serde(default)]
    pub export_dir: Option<String>,
}

fn default_raw_hours() -> u64 {
    24
}

fn default_daily_keep_days() -> u32 {
    31
}

fn default_monthly_keep_months() -> u32 {
    12
}

fn default_yearly_keep_years() -> u32 {
    7
}

impl Default for TieredRetentionConfig {
    fn default() -> Self {
        Self {
            raw_hours: 24,
            daily_keep_days: 31,
            monthly_keep_months: 12,
            yearly_keep_years: 7,
            cleanup_interval_hours: 24,
            export_before_delete: false,
            export_dir: None,
        }
    }
}

#[derive(Debug)]
pub struct TieredCleanupReport {
    pub raw_aggregated: u64,
    pub raw_deleted: u64,
    pub daily_aggregated: u64,
    pub daily_deleted: u64,
    pub monthly_aggregated: u64,
    pub monthly_deleted: u64,
    pub yearly_deleted: u64,
    pub exported_count: u64,
    pub export_path: Option<PathBuf>,
}
