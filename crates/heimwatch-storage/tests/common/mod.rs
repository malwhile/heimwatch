//! Common test utilities for heimwatch-storage integration tests.

use heimwatch_storage::{CpuData, MetricPayload, MetricRecord, NetworkData, StorageLayer};
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
    usage_percent: f32,
    core_count: u32,
}

impl CpuMetricBuilder {
    /// Create a new CPU metric builder with required fields.
    pub fn new(app_name: impl Into<String>, timestamp: u64, usage_percent: f32) -> Self {
        Self {
            app_name: app_name.into(),
            timestamp,
            usage_percent,
            core_count: 4,
        }
    }

    /// Set the core count (default: 4).
    #[allow(dead_code)]
    pub fn with_core_count(mut self, count: u32) -> Self {
        self.core_count = count;
        self
    }

    /// Build the metric record.
    pub fn build(self) -> MetricRecord {
        MetricRecord {
            app_name: self.app_name,
            timestamp: self.timestamp,
            payload: MetricPayload::Cpu(CpuData {
                usage_percent: self.usage_percent,
                core_count: self.core_count,
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

/// Helper for creating a CPU metric record with default core count.
pub fn make_cpu_record(app_name: &str, timestamp: u64, usage_percent: f32) -> MetricRecord {
    CpuMetricBuilder::new(app_name, timestamp, usage_percent).build()
}

/// Helper for creating a network metric record with default connections.
pub fn make_network_record(app_name: &str, timestamp: u64, tx: u64, rx: u64) -> MetricRecord {
    NetworkMetricBuilder::new(app_name, timestamp, tx, rx).build()
}

/// Helper for inserting multiple records at once.
#[allow(dead_code)]
pub fn insert_records(db: &StorageLayer, records: &[MetricRecord]) {
    for record in records {
        db.insert_metric(record).expect("Failed to insert metric");
    }
}
