//! Storage layer implementation using sled.
//!
//! # Async Note
//!
//! Sled is synchronous. When calling from an async context (e.g., in
//! `heimwatch-daemon` or `heimwatch-web`), wrap calls in:
//! ```ignore
//! tokio::task::spawn_blocking(move || storage.insert_metric(&record)).await?
//! ```

use crate::error::StorageError;
use crate::keys;
use anyhow::{Context, Result};
use heimwatch_core::{
    AppFocusStats, AppNetworkStats, AppPowerStats, CpuData, DiskData, FocusData, GpuProcessData,
    MemoryData, MetricPayload, MetricRecord, MetricType, NetworkData, PowerData,
    current_unix_timestamp,
};
use sled::Db;
use std::collections::HashMap;

const META_LAST_CLEANUP: &[u8; 17] = b"meta:last_cleanup";
const CONFIG_RETENTION_DAYS: &[u8; 21] = b"config:retention_days";

/// Main storage interface using sled.
///
/// The database uses a single `metrics` tree with keys formatted as:
/// `{prefix}:{timestamp:020}:{app_name}`
///
/// Configuration and metadata is stored in a separate `meta` tree.
pub struct StorageLayer {
    db: Db,
}

impl StorageLayer {
    /// Open or create a sled database at the given path.
    pub fn open(path: &str) -> Result<Self> {
        let config = sled::Config::new()
            .path(path)
            .cache_capacity(256 * 1024 * 1024) // 256 MB cache
            .flush_every_ms(Some(1000)); // Async flush every 1s

        let db = config.open()?;
        Ok(StorageLayer { db })
    }

    /// Get the metrics tree, creating it if necessary.
    fn metrics_tree(&self) -> Result<sled::Tree> {
        Ok(self.db.open_tree("metrics")?)
    }

    /// Get the metadata tree, creating it if necessary.
    fn meta_tree(&self) -> Result<sled::Tree> {
        Ok(self.db.open_tree("meta")?)
    }

    fn get_metadata<T: serde::de::DeserializeOwned>(
        &self,
        tree: &sled::Tree,
        key: &[u8],
        default: T,
    ) -> anyhow::Result<T> {
        match tree.get(key)? {
            Some(v) => serde_json::from_slice(&v).context("Metadata deserialization"),
            None => Ok(default),
        }
    }

    /// Insert a single metric record.
    pub fn insert_metric(&self, record: &MetricRecord) -> Result<()> {
        let tree = self.metrics_tree()?;
        let metric_type = record.payload.metric_type();
        let key = keys::encode_key(&metric_type, record.timestamp, &record.app_name);
        let value = serde_json::to_vec(record)?;
        tree.insert(key, value)?;
        Ok(())
    }

    /// Insert multiple metric records atomically.
    ///
    /// Uses a sled Batch for atomic writes and reduced I/O.
    pub fn insert_metrics_batch(&self, records: &[MetricRecord]) -> Result<()> {
        let tree = self.metrics_tree()?;
        let mut batch = sled::Batch::default();

        for record in records {
            let metric_type = record.payload.metric_type();
            let key = keys::encode_key(&metric_type, record.timestamp, &record.app_name);
            let value = serde_json::to_vec(record)?;
            batch.insert(key, value);
        }

        tree.apply_batch(batch)?;
        Ok(())
    }

    /// Retrieve all metrics of a specific type within a time range.
    pub fn get_metrics_by_type(
        &self,
        metric_type: MetricType,
        start: u64,
        end: u64,
    ) -> Result<Vec<MetricRecord>> {
        let tree = self.metrics_tree()?;
        let range_start = keys::range_start(&metric_type, start);
        let range_end = keys::range_end(&metric_type, end);

        let mut results = Vec::new();
        for item in tree.range(range_start..=range_end) {
            let (_key, value) = item?;
            let record: MetricRecord = serde_json::from_slice(&value)?;
            results.push(record);
        }
        Ok(results)
    }

    /// Retrieve all metrics for a specific app within a time range.
    ///
    /// **Note:** Since the key design is `{prefix}:{timestamp}:{app_name}`,
    /// efficient app-based lookups would require a secondary index.
    /// For now, we scan all metric types in the range and filter by app name.
    /// A future optimization: add a `pid_index` tree with keys `{app_name}:{type}:{timestamp}`.
    pub fn get_metrics_by_app(
        &self,
        app_name: &str,
        start: u64,
        end: u64,
    ) -> Result<Vec<MetricRecord>> {
        let mut results = Vec::new();

        // Scan all metric types
        for metric_type in heimwatch_core::ALL_METRIC_TYPES {
            let records = self.get_metrics_by_type(*metric_type, start, end)?;
            for record in records {
                if record.app_name == app_name {
                    results.push(record);
                }
            }
        }

        Ok(results)
    }

    /// Calculate the mean CPU time (in nanoseconds) for an app over a time range.
    pub fn get_aggregated_cpu(&self, app_name: &str, start: u64, end: u64) -> Result<f32> {
        let records = self.get_metrics_by_app(app_name, start, end)?;
        let cpu_records: Vec<u64> = records
            .iter()
            .filter_map(|r| match &r.payload {
                MetricPayload::Cpu(CpuData { cpu_time_ns, .. }) => Some(*cpu_time_ns),
                _ => None,
            })
            .collect();

        if cpu_records.is_empty() {
            return Err(StorageError::NotFound.into());
        }

        let mean = cpu_records.iter().sum::<u64>() as f32 / cpu_records.len() as f32;
        Ok(mean)
    }

    /// Get the top N apps by total network bytes (tx + rx) over a time range.
    pub fn get_top_apps_by_network(
        &self,
        start: u64,
        end: u64,
        limit: usize,
    ) -> Result<Vec<AppNetworkStats>> {
        let records = self.get_metrics_by_type(MetricType::Net, start, end)?;

        let mut app_totals: HashMap<String, (u64, u64)> = HashMap::new();

        for record in records {
            if let MetricPayload::Net(NetworkData {
                tx_bytes, rx_bytes, ..
            }) = record.payload
            {
                let (total_tx, total_rx) = app_totals.entry(record.app_name).or_insert((0, 0));
                *total_tx += tx_bytes;
                *total_rx += rx_bytes;
            }
        }

        let mut stats: Vec<AppNetworkStats> = app_totals
            .into_iter()
            .map(|(app_name, (tx_bytes, rx_bytes))| AppNetworkStats {
                app_name,
                tx_bytes,
                rx_bytes,
            })
            .collect();

        // Sort by total bytes (tx + rx) descending
        stats.sort_by(|a, b| {
            let a_total = a.tx_bytes + a.rx_bytes;
            let b_total = b.tx_bytes + b.rx_bytes;
            b_total.cmp(&a_total)
        });

        stats.truncate(limit);
        Ok(stats)
    }

    /// Delete records older than the retention period, returning the count deleted.
    pub fn cleanup_old_data(&self, retention_days: u32) -> Result<u64> {
        let now = current_unix_timestamp()?;

        let retention_seconds = retention_days as u64 * 86_400;
        let cutoff = now.saturating_sub(retention_seconds);

        let tree = self.metrics_tree()?;

        // Collect all keys to delete (can't delete while iterating).
        // Using tree.range() allows sled's B-tree to stop at the cutoff rather than
        // loading all matching keys into memory.
        let mut keys_to_delete = Vec::new();

        for metric_type in heimwatch_core::ALL_METRIC_TYPES {
            let range_start = keys::range_start(metric_type, 0);
            let range_end = keys::range_end(metric_type, cutoff);
            for item in tree.range(range_start..=range_end) {
                let (key, _) = item?;
                keys_to_delete.push(key.to_vec());
            }
        }

        let count = keys_to_delete.len() as u64;

        // Delete keys in a batch for atomicity.
        let mut batch = sled::Batch::default();
        for key in keys_to_delete {
            batch.remove(key);
        }
        tree.apply_batch(batch)?;

        Ok(count)
    }

    /// Set the retention period in days.
    pub fn set_retention_days(&self, days: u32) -> Result<()> {
        let tree = self.meta_tree()?;
        let value = serde_json::to_vec(&days)?;
        tree.insert(CONFIG_RETENTION_DAYS, value)?;
        Ok(())
    }

    /// Get the retention period in days, defaulting to 7 if not set.
    pub fn get_retention_days(&self) -> Result<u32> {
        let tree = self.meta_tree()?;
        self.get_metadata(&tree, CONFIG_RETENTION_DAYS, 7)
    }

    /// Set the timestamp of the last cleanup operation.
    pub fn set_last_cleanup_ts(&self, ts: u64) -> Result<()> {
        let tree = self.meta_tree()?;
        let value = serde_json::to_vec(&ts)?;
        tree.insert(META_LAST_CLEANUP, value)?;
        Ok(())
    }

    /// Get the timestamp of the last cleanup operation, if any.
    pub fn get_last_cleanup_ts(&self) -> Result<Option<u64>> {
        let tree = self.meta_tree()?;
        self.get_metadata(&tree, META_LAST_CLEANUP, None)
    }

    /// Manually flush the database transactions to disk
    pub fn flush(&self) -> Result<()> {
        self.db.flush()?;
        Ok(())
    }

    /// Insert a focus event (app focus duration).
    ///
    /// Constructs a MetricRecord and delegates to insert_metric.
    pub fn insert_focus_event(
        &self,
        app_name: &str,
        duration_ms: u64,
        timestamp: u64,
    ) -> Result<()> {
        let record = MetricRecord {
            app_name: app_name.to_string(),
            timestamp,
            payload: MetricPayload::Foc(FocusData {
                app_id: app_name.to_string(),
                duration_ms,
            }),
        };
        self.insert_metric(&record)
    }

    /// Get the top N apps by total focus time over a time range.
    pub fn get_top_apps_by_focus(
        &self,
        start: u64,
        end: u64,
        limit: usize,
    ) -> Result<Vec<AppFocusStats>> {
        let records = self.get_metrics_by_type(MetricType::Foc, start, end)?;

        let mut app_totals: HashMap<String, u64> = HashMap::new();

        for record in records {
            if let MetricPayload::Foc(FocusData { duration_ms, .. }) = record.payload {
                let total = app_totals.entry(record.app_name).or_insert(0);
                *total += duration_ms;
            }
        }

        let mut stats: Vec<AppFocusStats> = app_totals
            .into_iter()
            .map(|(app_name, total_duration_ms)| AppFocusStats {
                app_name,
                total_duration_ms,
            })
            .collect();

        // Sort by total duration descending
        stats.sort_by_key(|b| std::cmp::Reverse(b.total_duration_ms));

        stats.truncate(limit);
        Ok(stats)
    }

    /// Compute per-app power consumption statistics over a time range.
    ///
    /// Uses the fixed-weight power attribution formula from the power plan:
    /// - CPU: 0.40 (or RAPL-calibrated if available)
    /// - GPU: 0.20
    /// - Display (focus time): 0.15
    /// - Disk I/O: 0.10
    /// - Network: 0.10
    /// - Memory: 0.05
    ///
    /// Returns apps sorted descending by power_pct.
    pub fn get_power_stats(
        &self,
        start: u64,
        end: u64,
        on_battery: Option<bool>,
    ) -> Result<Vec<AppPowerStats>> {
        let window_ms = (end - start) * 1000;
        if window_ms == 0 {
            return Ok(Vec::new());
        }

        // Fetch all metric records in the range
        let cpu_records = self.get_metrics_by_type(MetricType::Cpu, start, end)?;
        let mem_records = self.get_metrics_by_type(MetricType::Mem, start, end)?;
        let dsk_records = self.get_metrics_by_type(MetricType::Dsk, start, end)?;
        let net_records = self.get_metrics_by_type(MetricType::Net, start, end)?;
        let gpu_records = self.get_metrics_by_type(MetricType::GpuProc, start, end)?;
        let foc_records = self.get_metrics_by_type(MetricType::Foc, start, end)?;
        let pwr_records = self.get_metrics_by_type(MetricType::Pwr, start, end)?;

        // Filter by battery state if requested
        let on_battery_set: std::collections::HashSet<u64> = if let Some(true) = on_battery {
            pwr_records
                .iter()
                .filter_map(|r| {
                    if let MetricPayload::Pwr(PowerData {
                        charging,
                        battery_percent,
                        ..
                    }) = &r.payload
                    {
                        if !charging && battery_percent.is_some() {
                            return Some(r.timestamp);
                        }
                    }
                    None
                })
                .collect()
        } else if let Some(false) = on_battery {
            pwr_records
                .iter()
                .filter_map(|r| {
                    if let MetricPayload::Pwr(PowerData { charging, .. }) = &r.payload {
                        if *charging {
                            return Some(r.timestamp);
                        }
                    }
                    None
                })
                .collect()
        } else {
            // No filter - all timestamps are valid
            (0..=u64::MAX).collect()
        };

        // Aggregate metrics per app
        #[derive(Default)]
        struct AppMetrics {
            cpu_pcts: Vec<f32>,
            gpu_pcts: Vec<f32>,
            focus_ms: u64,
            disk_bytes: u64,
            net_bytes: u64,
            mem_rss: Vec<u64>,
        }

        let mut app_metrics: HashMap<String, AppMetrics> = HashMap::new();

        // Process CPU
        for record in &cpu_records {
            if on_battery.is_none() || on_battery_set.contains(&record.timestamp) {
                if let MetricPayload::Cpu(CpuData {
                    cpu_usage_percent, ..
                }) = record.payload
                {
                    app_metrics
                        .entry(record.app_name.clone())
                        .or_default()
                        .cpu_pcts
                        .push(cpu_usage_percent);
                }
            }
        }

        // Process GPU
        for record in &gpu_records {
            if on_battery.is_none() || on_battery_set.contains(&record.timestamp) {
                if let MetricPayload::GpuProc(GpuProcessData {
                    usage_percent: Some(pct),
                    ..
                }) = record.payload
                {
                    app_metrics
                        .entry(record.app_name.clone())
                        .or_default()
                        .gpu_pcts
                        .push(pct);
                }
            }
        }

        // Process Focus
        for record in &foc_records {
            if on_battery.is_none() || on_battery_set.contains(&record.timestamp) {
                if let MetricPayload::Foc(FocusData { duration_ms, .. }) = record.payload {
                    app_metrics
                        .entry(record.app_name.clone())
                        .or_default()
                        .focus_ms += duration_ms;
                }
            }
        }

        // Process Disk
        for record in &dsk_records {
            if on_battery.is_none() || on_battery_set.contains(&record.timestamp) {
                if let MetricPayload::Dsk(DiskData {
                    read_bytes,
                    write_bytes,
                    ..
                }) = record.payload
                {
                    app_metrics
                        .entry(record.app_name.clone())
                        .or_default()
                        .disk_bytes += read_bytes + write_bytes;
                }
            }
        }

        // Process Network
        for record in &net_records {
            if on_battery.is_none() || on_battery_set.contains(&record.timestamp) {
                if let MetricPayload::Net(NetworkData {
                    tx_bytes, rx_bytes, ..
                }) = record.payload
                {
                    app_metrics
                        .entry(record.app_name.clone())
                        .or_default()
                        .net_bytes += tx_bytes + rx_bytes;
                }
            }
        }

        // Process Memory
        for record in &mem_records {
            if on_battery.is_none() || on_battery_set.contains(&record.timestamp) {
                if let MetricPayload::Mem(MemoryData { rss_bytes, .. }) = record.payload {
                    app_metrics
                        .entry(record.app_name.clone())
                        .or_default()
                        .mem_rss
                        .push(rss_bytes);
                }
            }
        }

        // Compute averages and normalization factors
        let max_disk_bytes = app_metrics
            .values()
            .map(|m| m.disk_bytes)
            .max()
            .unwrap_or(1);
        let max_net_bytes = app_metrics.values().map(|m| m.net_bytes).max().unwrap_or(1);
        let total_mem_rss: u64 = app_metrics.values().flat_map(|m| &m.mem_rss).sum();

        // Compute power scores (with intermediate scores for contribution tracking)
        #[derive(Default)]
        struct ScoreComponents {
            cpu: f32,
            gpu: f32,
            display: f32,
            disk: f32,
            net: f32,
            mem: f32,
        }

        let power_stats: Vec<(AppPowerStats, ScoreComponents)> = app_metrics
            .into_iter()
            .map(|(app_name, metrics)| {
                let cpu_pct_avg = if metrics.cpu_pcts.is_empty() {
                    0.0
                } else {
                    metrics.cpu_pcts.iter().sum::<f32>() / metrics.cpu_pcts.len() as f32
                };

                let gpu_pct_max = metrics.gpu_pcts.iter().cloned().fold(0.0, f32::max);

                let focus_fraction = (metrics.focus_ms as f32 / window_ms as f32) * 100.0;
                let focus_fraction = focus_fraction.clamp(0.0, 100.0);

                let disk_normalized = if max_disk_bytes > 0 {
                    ((metrics.disk_bytes as f32 + 1.0).log2()
                        / (max_disk_bytes as f32 + 1.0).log2())
                        * 100.0
                } else {
                    0.0
                };

                let net_normalized = if max_net_bytes > 0 {
                    ((metrics.net_bytes as f32 + 1.0).log2() / (max_net_bytes as f32 + 1.0).log2())
                        * 100.0
                } else {
                    0.0
                };

                let mem_fraction = if total_mem_rss > 0 {
                    let mem_rss_avg = if metrics.mem_rss.is_empty() {
                        0
                    } else {
                        metrics.mem_rss.iter().sum::<u64>() / metrics.mem_rss.len() as u64
                    };
                    (mem_rss_avg as f32 / total_mem_rss as f32) * 100.0
                } else {
                    0.0
                };

                // Compute weighted components
                let components = ScoreComponents {
                    cpu: 0.40 * cpu_pct_avg,
                    gpu: 0.20 * gpu_pct_max,
                    display: 0.15 * focus_fraction,
                    disk: 0.10 * disk_normalized,
                    net: 0.10 * net_normalized,
                    mem: 0.05 * mem_fraction,
                };

                let power_score = components.cpu
                    + components.gpu
                    + components.display
                    + components.disk
                    + components.net
                    + components.mem;

                let stat = AppPowerStats {
                    app_name,
                    power_pct: 0.0, // Normalized later
                    power_score,
                    on_battery: on_battery.unwrap_or(false),
                    cpu_contribution: 0.0,
                    gpu_contribution: 0.0,
                    display_contribution: 0.0,
                    disk_contribution: 0.0,
                    net_contribution: 0.0,
                    mem_contribution: 0.0,
                };

                (stat, components)
            })
            .collect();

        // Normalize scores to percentages and compute contributions
        let total_score: f32 = power_stats.iter().map(|(s, _)| s.power_score).sum();
        let mut final_stats = Vec::new();
        for (mut stat, components) in power_stats {
            if total_score > 0.0 {
                stat.power_pct = (stat.power_score / total_score) * 100.0;
                if stat.power_score > 0.0 {
                    stat.cpu_contribution = components.cpu / stat.power_score;
                    stat.gpu_contribution = components.gpu / stat.power_score;
                    stat.display_contribution = components.display / stat.power_score;
                    stat.disk_contribution = components.disk / stat.power_score;
                    stat.net_contribution = components.net / stat.power_score;
                    stat.mem_contribution = components.mem / stat.power_score;
                }
            }
            final_stats.push(stat);
        }

        let mut power_stats = final_stats;

        // Sort by power_pct descending
        power_stats.sort_by(|a, b| {
            b.power_pct
                .partial_cmp(&a.power_pct)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(power_stats)
    }
}
