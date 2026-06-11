//! Common test utilities for heimwatch-storage integration tests.

use heimwatch_storage::{
    CpuData, FocusData, MetricPayload, MetricRecord, NetworkData, StorageLayer,
};
use tempfile::TempDir;

/// Creates a temporary test database for use in tests.
///
/// Returns a tuple of (StorageLayer, TempDir). The TempDir is returned
/// to keep it alive for the duration of the test.
///
/// # Example
/// ```ignore
/// #[test]
/// fn test_something() {
///     let (db, _tmpdir) = create_test_db();
///     // Use db...
/// }
/// ```
pub fn create_test_db() -> (StorageLayer, TempDir) {
    let tmpdir = TempDir::new().expect("Failed to create temp directory");
    let db = StorageLayer::open(tmpdir.path().to_str().expect("Invalid temp path"))
        .expect("Failed to open storage layer");
    (db, tmpdir)
}

/// Builder for creating test CPU metrics.
pub struct CpuMetricBuilder {
    app_name: String,
    timestamp: u64,
    cpu_time_ns: u64,
    cpu_usage_percent: f32,
}

impl CpuMetricBuilder {
    /// Create a new CPU metric builder with required fields.
    /// cpu_time_ns is the absolute CPU time in nanoseconds.
    pub fn new(app_name: impl Into<String>, timestamp: u64, cpu_time_ns: u64) -> Self {
        Self {
            app_name: app_name.into(),
            timestamp,
            cpu_time_ns,
            cpu_usage_percent: 0.0,
        }
    }

    /// Set the CPU usage percentage (default: 0.0).
    #[allow(dead_code)]
    pub fn with_usage_percent(mut self, percent: f32) -> Self {
        self.cpu_usage_percent = percent;
        self
    }

    /// Build the metric record.
    pub fn build(self) -> MetricRecord {
        MetricRecord {
            app_name: self.app_name,
            timestamp: self.timestamp,
            payload: MetricPayload::Cpu(CpuData {
                cpu_time_ns: self.cpu_time_ns,
                cpu_usage_percent: self.cpu_usage_percent,
                thread_count: 1,
            }),
        }
    }
}

/// Builder for creating test network metrics.
pub struct NetworkMetricBuilder {
    app_name: String,
    timestamp: u64,
    tx_bytes: u64,
    rx_bytes: u64,
    connections: u32,
}

impl NetworkMetricBuilder {
    /// Create a new network metric builder with required fields.
    pub fn new(app_name: impl Into<String>, timestamp: u64, tx_bytes: u64, rx_bytes: u64) -> Self {
        Self {
            app_name: app_name.into(),
            timestamp,
            tx_bytes,
            rx_bytes,
            connections: 1,
        }
    }

    /// Set the number of connections (default: 1).
    #[allow(dead_code)]
    pub fn with_connections(mut self, count: u32) -> Self {
        self.connections = count;
        self
    }

    /// Build the metric record.
    pub fn build(self) -> MetricRecord {
        MetricRecord {
            app_name: self.app_name,
            timestamp: self.timestamp,
            payload: MetricPayload::Net(NetworkData {
                tx_bytes: self.tx_bytes,
                rx_bytes: self.rx_bytes,
                connections: self.connections,
            }),
        }
    }
}

/// Helper for creating a CPU metric record.
/// cpu_time_ns is the absolute CPU time in nanoseconds.
pub fn make_cpu_record(app_name: &str, timestamp: u64, cpu_time_ns: u64) -> MetricRecord {
    CpuMetricBuilder::new(app_name, timestamp, cpu_time_ns).build()
}

/// Helper for creating a network metric record with default connections.
pub fn make_network_record(app_name: &str, timestamp: u64, tx: u64, rx: u64) -> MetricRecord {
    NetworkMetricBuilder::new(app_name, timestamp, tx, rx).build()
}

/// Builder for creating test focus metrics.
pub struct FocusMetricBuilder {
    app_name: String,
    timestamp: u64,
    duration_ms: u64,
}

impl FocusMetricBuilder {
    /// Create a new focus metric builder with required fields.
    pub fn new(app_name: impl Into<String>, timestamp: u64, duration_ms: u64) -> Self {
        Self {
            app_name: app_name.into(),
            timestamp,
            duration_ms,
        }
    }

    /// Build the metric record.
    pub fn build(self) -> MetricRecord {
        MetricRecord {
            app_name: self.app_name.clone(),
            timestamp: self.timestamp,
            payload: MetricPayload::Foc(FocusData {
                app_id: self.app_name,
                duration_ms: self.duration_ms,
            }),
        }
    }
}

/// Helper for creating a focus metric record.
pub fn make_focus_record(app_name: &str, timestamp: u64, duration_ms: u64) -> MetricRecord {
    FocusMetricBuilder::new(app_name, timestamp, duration_ms).build()
}

/// Helper for inserting multiple records at once.
#[allow(dead_code)]
pub fn insert_records(db: &StorageLayer, records: &[MetricRecord]) {
    for record in records {
        db.insert_metric(record).expect("Failed to insert metric");
    }
}
