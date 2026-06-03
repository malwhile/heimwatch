# Data Retention and Cleanup

Heimwatch automatically manages database storage by deleting old metric records based on configurable retention policies. This prevents unbounded database growth while allowing flexible per-metric-type retention periods.

## Overview

- **Automatic cleanup**: Background daemon task runs periodically to delete expired records
- **Per-metric-type retention**: CPU, network, disk, memory, power, focus, and GPU metrics can have different retention periods
- **Export before delete**: Optionally export records to JSONL before deletion for archival
- **Storage monitoring**: Log storage statistics (database size, record counts) after each cleanup

## Configuration

Create a `heimwatch.toml` file in the daemon's working directory:

```toml
[retention]
# Data retention periods in days for each metric type (default: 7 days)
cpu_days = 7
net_days = 7
pwr_days = 7
foc_days = 7
mem_days = 7
dsk_days = 7
gpu_days = 7
gpu_proc_days = 7

# Cleanup interval in hours (default: 24)
cleanup_interval_hours = 24

# Export records before deletion (default: false)
export_before_delete = false

# Export directory path (only used if export_before_delete = true)
# export_dir = "./heimwatch-exports"
```

## Daemon Invocation

```bash
# Use default config (./heimwatch.toml) or specify a custom path
./heimwatch-daemon daemon --db ./heimwatch.db --config ./heimwatch.toml

# If config file doesn't exist, defaults are used (all 7 days, 24-hour cleanup)
./heimwatch-daemon daemon --db ./heimwatch.db
```

## How It Works

1. **On startup**: Daemon loads retention config from TOML file (or uses defaults if missing)
2. **Periodically**: Background cleanup task wakes up every `cleanup_interval_hours`
3. **Export (optional)**: If `export_before_delete = true`, all expired records are written to a JSONL file
4. **Delete**: For each metric type, records older than its configured retention period are deleted atomically
5. **Log**: Storage stats (db size, record counts per type) are logged after cleanup

## Defaults

| Metric Type | Default Retention | Purpose |
|---|---|---|
| CPU | 7 days | Raw CPU usage per process |
| Network | 7 days | Raw network bytes per process |
| Power | 7 days | Raw power consumption per process |
| Focus | 7 days | Raw window focus events |
| Memory | 7 days | Raw memory usage per process |
| Disk | 7 days | Raw disk I/O per process |
| GPU | 7 days | Raw GPU metrics |
| GPU Process | 7 days | Raw per-process GPU metrics |

## Export Format

When `export_before_delete = true`, records are exported to:

```
export_dir/export-{TIMESTAMP}.jsonl
```

Each line is a JSON-serialized `MetricRecord`:

```json
{"app_name":"firefox","timestamp":1711890000,"payload":{"Cpu":{"cpu_time_ns":1000000000,"cpu_usage_percent":50.0}}}
{"app_name":"chrome","timestamp":1711890000,"payload":{"Net":{"tx_bytes":1000000,"rx_bytes":2000000,"connections":5}}}
```

## Cleanup Report

The daemon logs cleanup summary after each run:

```
INFO: Cleanup: deleted=150000, exported=150000
INFO: Storage: 536870912 bytes, 5000000 total records
```

## Future: Aggregation Tiers

The config file reserves fields for hourly and daily aggregation (not yet implemented):

```toml
# Reserved for future aggregation support
[retention]
# ... raw fields (as above)
hourly_retention_days = 30  # Not yet implemented
daily_retention_days = 365  # Not yet implemented
```

When aggregation is implemented, raw data will be summarized into hourly and daily buckets with independent retention.

## API Usage

```rust
use heimwatch_storage::{StorageLayer, RetentionConfig};

let storage = StorageLayer::open("./heimwatch.db")?;

// Configure retention
let mut config = RetentionConfig::default();
config.cpu_days = 14;  // Keep CPU data for 2 weeks
config.net_days = 7;   // Keep network for 1 week

// Run cleanup with config
let report = storage.cleanup_with_config(&config)?;
println!("Deleted {} records", report.deleted_count);

// Get storage stats
let stats = storage.get_storage_stats()?;
println!("DB size: {} bytes", stats.db_size_bytes);
println!("CPU records: {}", stats.record_counts[&MetricType::Cpu]);
```

## Testing

Run retention tests:

```bash
# Storage layer tests
cargo test -p heimwatch-storage retention

# Daemon config tests
cargo test -p heimwatch-daemon retention
```
