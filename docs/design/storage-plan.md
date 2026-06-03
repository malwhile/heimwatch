# Specification: Sled Database Interface for Heimwatch

## 1. Objective
Implement the heimwatch-storage crate to persist time-series monitoring data using the sled embedded database. The interface must be efficient for high-frequency writes (append-only), support time-range queries, and handle data retention.

## 2. Design Constraints
- Engine: sled (v0.34+)
- Serialization: serde_json (for flexibility) or bincode (for performance). Decision: Use serde_json for human-readable debugging during dev, but optimize for bincode if perf becomes an issue.
- Concurrency: Thread-safe access (sled is thread-safe by default).
- Performance: Must handle writes every 5 seconds from multiple collectors without blocking the main loop.
- Portability: Must work on Linux, macOS, Windows, and BSD (sled is cross-platform).

## 3. Data Schema Design

We will use a Key-Value structure optimized for time-series retrieval.

### 3.1. Key Naming Convention
Keys will be prefixed by metric type to allow efficient range scans.
Format: {metric_type}:{timestamp_epoch}:{pid}

| Metric Type | Prefix | Example Key |
| :--- | :--- | :--- |
| Network | net: | net:1711890000:1234 |
| Power | pwr: | pwr:1711890000:1234 |
| Focus | foc: | foc:1711890000:1234 |
| CPU | cpu: | cpu:1711890000:1234 |
| Memory | mem: | mem:1711890000:1234 |
| Disk | dsk: | dsk:1711890000:1234 |
| GPU | gpu: | gpu:1711890000:1234 |

Note: timestamp_epoch is the Unix timestamp (seconds) to ensure chronological ordering in the B-Tree.

### 3.2. Value Structure
Values will be JSON objects containing the metric data.

Example: Network Metric
{
  "tx_bytes": 1024,
  "rx_bytes": 2048,
  "connections": 5
}

Example: Focus Metric
{
  "window_title": "Visual Studio Code",
  "duration_seconds": 300
}

### 3.3. Metadata Keys (Non-Time Series)
- config:retention_days: Integer (default 7)
- config:alert_rules: JSON array of rules
- meta:last_cleanup: Timestamp

## 4. Required API Surface (StorageLayer Trait)

The crate must expose a StorageLayer struct that wraps the sled::Db.

### 4.1. Initialization
- open(path: &str) -> Result<Self, StorageError>
  - Opens or creates the database.
  - Configures sled for high throughput (e.g., flush_every_ms(None) for async flush, or specific cache size).

### 4.2. Write Operations (Batching)
- insert_metric(metric: MetricRecord) -> Result<(), StorageError>
  - Serializes the record and inserts it.
  - Optimization: Should support batching multiple inserts before flushing to disk to reduce I/O.

### 4.3. Read Operations (Time Range)
- get_metrics_by_pid(pid: u32, start: u64, end: u64) -> Result<Vec<MetricRecord>, StorageError>
  - Returns all records for a specific PID within a time window.
  - Implementation: Uses tree.scan_prefix() or tree.range() with key construction.

- get_metrics_by_type(metric_type: MetricType, start: u64, end: u64) -> Result<Vec<MetricRecord>, StorageError>
  - Returns all records of a specific type (e.g., all CPU usage) across all PIDs.

### 4.4. Aggregation Helpers
- get_aggregated_cpu(pid: u32, start: u64, end: u64) -> Result<f32, StorageError>
  - Calculates average CPU usage for a PID in a range.
- get_top_apps_by_network(start: u64, end: u64, limit: usize) -> Result<Vec<AppNetworkStats>, StorageError>
  - Aggregates network bytes per PID and returns top N.

### 4.5. Maintenance
- cleanup_old_data(retention_days: u32) -> Result<u64, StorageError>
  - Scans for keys older than now - retention_days and deletes them.
  - Returns the number of records deleted.

## 5. Implementation Details & Edge Cases

### 5.1. Handling Process Restart (PID Reuse)
- If a process dies and a new one gets the same PID, the keys will overlap.
- Strategy: The MetricRecord struct must include a process_start_time or unique_instance_id if possible.
- Fallback: If not available, the system assumes the new process starts with 0 bytes. The cleanup logic must handle gaps.

### 5.2. Error Handling
- Define a custom StorageError enum:
  - IoError(std::io::Error)
  - SerializationError(serde_json::Error)
  - DatabaseClosed
  - NotFound

### 5.3. Configuration
- Allow configuring sled via Config:
  - cache_capacity: Set to ~256MB for desktop usage.
  - mode: Mode::HighThroughput (async writes).

## 6. Deliverables

1. crates/heimwatch-storage/Cargo.toml:
   - Dependencies: sled, serde, serde_json, thiserror, chrono.
2. crates/heimwatch-storage/src/lib.rs:
   - Public API exports.
3. crates/heimwatch-storage/src/db.rs:
   - StorageLayer implementation.
4. crates/heimwatch-storage/src/models.rs:
   - MetricRecord, MetricType, NetworkData, PowerData, etc.
5. crates/heimwatch-storage/src/error.rs:
   - StorageError definition.
6. crates/heimwatch-storage/tests/integration_test.rs:
   - Test writing a record, reading it back, and verifying time-range queries.
   - Test cleanup_old_data.

## 7. Instructions for Generation

Please generate the code for the heimwatch-storage crate following these steps:
1. Define the models.rs with all metric structs and the MetricType enum.
2. Define the error.rs.
3. Implement the StorageLayer in db.rs with the methods listed in Section 4.
4. Ensure the key generation logic is robust (e.g., using format!("{:?}:{}:{}", ...)).
5. Add a main.rs example in the crate to demonstrate opening the DB and inserting a dummy record.
6. Write the integration tests.

Note: Ensure the code is no_std compatible? No, this is a desktop app, so std is fine. Focus on async friendliness if possible, but sled is sync-friendly, so we will wrap it in tokio::task::spawn_blocking if used in an async context.
