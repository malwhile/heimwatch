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
use heimwatch_core::{
    AppNetworkStats, CpuData, MetricPayload, MetricRecord, MetricType, NetworkData,
};
use sled::Db;
use std::collections::HashMap;

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
    pub fn open(path: &str) -> Result<Self, StorageError> {
        let config = sled::Config::new()
            .path(path)
            .cache_capacity(256 * 1024 * 1024) // 256 MB cache
            .flush_every_ms(Some(1000)); // Async flush every 1s

        let db = config.open()?;
        Ok(StorageLayer { db })
    }

    /// Get the metrics tree, creating it if necessary.
    fn metrics_tree(&self) -> Result<sled::Tree, StorageError> {
        Ok(self.db.open_tree("metrics")?)
    }

    /// Get the metadata tree, creating it if necessary.
    fn meta_tree(&self) -> Result<sled::Tree, StorageError> {
        Ok(self.db.open_tree("meta")?)
    }

    /// Insert a single metric record.
    pub fn insert_metric(&self, record: &MetricRecord) -> Result<(), StorageError> {
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
    pub fn insert_metrics_batch(&self, records: &[MetricRecord]) -> Result<(), StorageError> {
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
    ) -> Result<Vec<MetricRecord>, StorageError> {
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
    ) -> Result<Vec<MetricRecord>, StorageError> {
        let mut results = Vec::new();

        // Scan all metric types
        for metric_type in [
            MetricType::Net,
            MetricType::Pwr,
            MetricType::Foc,
            MetricType::Cpu,
            MetricType::Mem,
            MetricType::Dsk,
            MetricType::Gpu,
        ] {
            let records = self.get_metrics_by_type(metric_type, start, end)?;
            for record in records {
                if record.app_name == app_name {
                    results.push(record);
                }
            }
        }

        Ok(results)
    }

    /// Calculate the mean CPU usage for an app over a time range.
    pub fn get_aggregated_cpu(
        &self,
        app_name: &str,
        start: u64,
        end: u64,
    ) -> Result<f32, StorageError> {
        let records = self.get_metrics_by_app(app_name, start, end)?;
        let cpu_records: Vec<f32> = records
            .iter()
            .filter_map(|r| match &r.payload {
                MetricPayload::Cpu(CpuData { usage_percent, .. }) => Some(*usage_percent),
                _ => None,
            })
            .collect();

        if cpu_records.is_empty() {
            return Err(StorageError::NotFound);
        }

        let mean = cpu_records.iter().sum::<f32>() / cpu_records.len() as f32;
        Ok(mean)
    }

    /// Get the top N apps by total network bytes (tx + rx) over a time range.
    pub fn get_top_apps_by_network(
        &self,
        start: u64,
        end: u64,
        limit: usize,
    ) -> Result<Vec<AppNetworkStats>, StorageError> {
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
    pub fn cleanup_old_data(&self, retention_days: u32) -> Result<u64, StorageError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| StorageError::SystemTimeError)?
            .as_secs();

        let retention_seconds = retention_days as u64 * 86_400;
        let cutoff = now.saturating_sub(retention_seconds);

        let tree = self.metrics_tree()?;

        // Collect all keys to delete (can't delete while iterating).
        // Using tree.range() allows sled's B-tree to stop at the cutoff rather than
        // loading all matching keys into memory.
        let mut keys_to_delete = Vec::new();

        for metric_type in [
            MetricType::Net,
            MetricType::Pwr,
            MetricType::Foc,
            MetricType::Cpu,
            MetricType::Mem,
            MetricType::Dsk,
            MetricType::Gpu,
        ] {
            let range_start = keys::range_start(&metric_type, 0);
            let range_end = keys::range_end(&metric_type, cutoff);
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
    pub fn set_retention_days(&self, days: u32) -> Result<(), StorageError> {
        let tree = self.meta_tree()?;
        let value = serde_json::to_vec(&days)?;
        tree.insert(b"config:retention_days", value)?;
        Ok(())
    }

    /// Get the retention period in days, defaulting to 7 if not set.
    pub fn get_retention_days(&self) -> Result<u32, StorageError> {
        let tree = self.meta_tree()?;
        match tree.get(b"config:retention_days")? {
            Some(value) => Ok(serde_json::from_slice(&value)?),
            None => Ok(7),
        }
    }

    /// Set the timestamp of the last cleanup operation.
    pub fn set_last_cleanup_ts(&self, ts: u64) -> Result<(), StorageError> {
        let tree = self.meta_tree()?;
        let value = serde_json::to_vec(&ts)?;
        tree.insert(b"meta:last_cleanup", value)?;
        Ok(())
    }

    /// Get the timestamp of the last cleanup operation, if any.
    pub fn get_last_cleanup_ts(&self) -> Result<Option<u64>, StorageError> {
        let tree = self.meta_tree()?;
        match tree.get(b"meta:last_cleanup")? {
            Some(value) => Ok(Some(serde_json::from_slice(&value)?)),
            None => Ok(None),
        }
    }
}
