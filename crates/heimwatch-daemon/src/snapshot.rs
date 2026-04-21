//! One-shot metric snapshots: network traffic, CPU usage, disk I/O, and focus time.

use anyhow::{Result, anyhow};
use heimwatch_collector::PlatformCollector;
use heimwatch_core::{MetricPayload, MetricRecord, current_unix_timestamp};
use heimwatch_storage::StorageLayer;
use std::collections::HashMap;
use std::time::Duration;

/// Capture a snapshot of metrics (network, CPU, disk, or focus) and print results.
///
/// Dispatches to platform-specific collectors or database queries based on metric_type.
pub async fn run_snapshot(
    window_secs: u64,
    format: &str,
    metric_type: &str,
    db_path: Option<&str>,
) -> Result<()> {
    match metric_type {
        "cpu" => run_cpu_snapshot(window_secs, format).await,
        "disk" => run_disk_snapshot(window_secs, format).await,
        "focus" => run_focus_snapshot(window_secs, format, db_path).await,
        "network" => run_network_snapshot(window_secs, format).await,
        _ => anyhow::bail!("Please choose cpu, disk, focus, or network"),
    }
}

/// Capture network traffic snapshot (one-shot probe collection).
async fn run_network_snapshot(window_secs: u64, format: &str) -> Result<()> {
    if window_secs == 0 {
        anyhow::bail!("Window must be greater than 0 seconds");
    }

    log::info!("Attaching eBPF probes, observing for {}s...", window_secs);
    let mut collector = tokio::task::spawn_blocking(PlatformCollector::new).await??;
    let mut network_collector = collector
        .take_network_collector()
        .ok_or_else(|| anyhow!("Network collection unavailable on this platform"))?;

    tokio::time::sleep(Duration::from_secs(window_secs)).await;
    let records =
        tokio::task::spawn_blocking(move || network_collector.collect_network()).await??;

    format_output(format, &records, window_secs)?;
    Ok(())
}

/// Capture CPU usage snapshot (one-shot probe collection).
async fn run_cpu_snapshot(window_secs: u64, format: &str) -> Result<()> {
    if window_secs == 0 {
        anyhow::bail!("Window must be greater than 0 seconds");
    }

    log::info!(
        "Attaching eBPF sched_switch probe, observing for {}s...",
        window_secs
    );
    let mut collector = tokio::task::spawn_blocking(PlatformCollector::new).await??;
    let mut cpu_collector = collector
        .take_cpu_collector()
        .ok_or_else(|| anyhow!("CPU tracking unavailable on this platform"))?;

    tokio::time::sleep(Duration::from_secs(window_secs)).await;
    let records = tokio::task::spawn_blocking(move || {
        cpu_collector.collect_cpu(Duration::from_secs(window_secs))
    })
    .await??;

    format_output(format, &records, window_secs)?;
    Ok(())
}

/// Capture disk I/O snapshot (one-shot probe collection).
async fn run_disk_snapshot(window_secs: u64, format: &str) -> Result<()> {
    if window_secs == 0 {
        anyhow::bail!("Window must be greater than 0 seconds");
    }

    log::info!(
        "Attaching eBPF block_rq_issue probe, observing for {}s...",
        window_secs
    );
    let mut collector = tokio::task::spawn_blocking(PlatformCollector::new).await??;
    let mut disk_collector = collector
        .take_disk_collector()
        .ok_or_else(|| anyhow!("Disk I/O tracking unavailable on this platform"))?;

    tokio::time::sleep(Duration::from_secs(window_secs)).await;
    let records = tokio::task::spawn_blocking(move || {
        disk_collector.collect_disk(Duration::from_secs(window_secs))
    })
    .await??;

    format_output(format, &records, window_secs)?;
    Ok(())
}

/// Capture focus time snapshot (database query).
async fn run_focus_snapshot(window_secs: u64, format: &str, db_path: Option<&str>) -> Result<()> {
    let db_path = db_path.ok_or_else(|| anyhow!("--db argument required for focus snapshot"))?;

    log::info!("Querying focus events from last {}s...", window_secs);
    let storage =
        StorageLayer::open(db_path).map_err(|e| anyhow!("Failed to open database: {}", e))?;

    let now = current_unix_timestamp()?;
    let start = now.saturating_sub(window_secs);
    let end = now;
    let records = storage
        .get_metrics_by_type(heimwatch_core::MetricType::Foc, start, end)
        .map_err(|e| anyhow!("Failed to query focus events: {}", e))?;

    format_output(format, &records, window_secs)?;
    Ok(())
}

/// Format output: JSON or human-readable table.
fn format_output(format: &str, records: &[MetricRecord], window_secs: u64) -> Result<()> {
    match format {
        "json" => {
            println!("{}", serde_json::to_string_pretty(records)?);
        }
        _ => {
            print_snapshot_table(records, window_secs);
        }
    }
    Ok(())
}

/// Print a human-readable table for any metric snapshot.
fn print_snapshot_table(records: &[MetricRecord], window_secs: u64) {
    if records.is_empty() {
        println!("\n  (no data)");
        println!();
        return;
    }

    // Determine table type from first record's payload
    match &records[0].payload {
        MetricPayload::Net(_) => print_network_table(records, window_secs),
        MetricPayload::Cpu(_) => print_cpu_table(records, window_secs),
        MetricPayload::Dsk(_) => print_disk_table(records, window_secs),
        MetricPayload::Foc(_) => print_focus_table(records, window_secs),
        _ => println!("  (unsupported metric type)"),
    }
}

/// Print a human-readable table of network traffic by application.
fn print_network_table(records: &[MetricRecord], window_secs: u64) {
    println!("\nHeiwatch Network Snapshot ({}s window)", window_secs);
    println!("{}", "─".repeat(52));
    println!("  {:<28} {:>10} {:>10}", "App", "TX", "RX");
    println!("  {}", "─".repeat(48));

    let (mut total_tx, mut total_rx) = (0u64, 0u64);
    for r in records {
        if let MetricPayload::Net(net) = &r.payload {
            println!(
                "  {:<28} {:>10} {:>10}",
                &r.app_name[..r.app_name.len().min(28)],
                fmt_bytes(net.tx_bytes),
                fmt_bytes(net.rx_bytes)
            );
            total_tx += net.tx_bytes;
            total_rx += net.rx_bytes;
        }
    }
    println!("{}", "─".repeat(52));
    println!(
        "  {:<28} {:>10} {:>10}",
        "Total",
        fmt_bytes(total_tx),
        fmt_bytes(total_rx)
    );
    println!();
}

/// Print a human-readable table of CPU usage by application.
fn print_cpu_table(records: &[MetricRecord], window_secs: u64) {
    println!("\nHeiwatch CPU Usage Snapshot ({}s window)", window_secs);
    println!("{}", "─".repeat(52));
    println!("  {:<28} {:>20}", "App", "CPU Usage");
    println!("  {}", "─".repeat(48));

    // Sort by usage descending
    let mut sorted: Vec<_> = records.iter().collect();
    sorted.sort_by(|a, b| {
        let usage_a = if let MetricPayload::Cpu(cpu) = &a.payload {
            cpu.usage_percent
        } else {
            0.0
        };
        let usage_b = if let MetricPayload::Cpu(cpu) = &b.payload {
            cpu.usage_percent
        } else {
            0.0
        };
        usage_b
            .partial_cmp(&usage_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut total_usage = 0.0;
    for r in sorted {
        if let MetricPayload::Cpu(cpu) = &r.payload {
            println!(
                "  {:<28} {:>19.1}%",
                &r.app_name[..r.app_name.len().min(28)],
                cpu.usage_percent
            );
            total_usage += cpu.usage_percent;
        }
    }
    println!("{}", "─".repeat(52));
    println!("  {:<28} {:>19.1}%", "Total", total_usage);
    println!();
}

/// Print a human-readable table of disk I/O by application.
fn print_disk_table(records: &[MetricRecord], window_secs: u64) {
    println!("\nHeiwatch Disk I/O Snapshot ({}s window)", window_secs);
    println!("{}", "─".repeat(64));
    println!("  {:<28} {:>10} {:>10}", "App", "Read", "Write");
    println!("  {}", "─".repeat(60));

    // Sort by total I/O descending
    let mut sorted: Vec<_> = records.iter().collect();
    sorted.sort_by(|a, b| {
        let total_a = if let MetricPayload::Dsk(dsk) = &a.payload {
            dsk.read_bytes + dsk.write_bytes
        } else {
            0
        };
        let total_b = if let MetricPayload::Dsk(dsk) = &b.payload {
            dsk.read_bytes + dsk.write_bytes
        } else {
            0
        };
        total_b.cmp(&total_a)
    });

    let (mut total_read, mut total_write) = (0u64, 0u64);
    for r in sorted {
        if let MetricPayload::Dsk(dsk) = &r.payload {
            println!(
                "  {:<28} {:>10} {:>10}",
                &r.app_name[..r.app_name.len().min(28)],
                fmt_bytes(dsk.read_bytes),
                fmt_bytes(dsk.write_bytes)
            );
            total_read += dsk.read_bytes;
            total_write += dsk.write_bytes;
        }
    }
    println!("{}", "─".repeat(64));
    println!(
        "  {:<28} {:>10} {:>10}",
        "Total",
        fmt_bytes(total_read),
        fmt_bytes(total_write)
    );
    println!();
}

/// Print a human-readable table of focus time by application.
fn print_focus_table(records: &[MetricRecord], window_secs: u64) {
    println!("\nHeiwatch Focus Time Snapshot ({}s window)", window_secs);
    println!("{}", "─".repeat(52));
    println!("  {:<28} {:>20}", "App", "Focus Time");
    println!("  {}", "─".repeat(48));

    // Aggregate focus time by app
    let mut app_totals: HashMap<String, u64> = HashMap::new();
    for r in records {
        if let MetricPayload::Foc(foc) = &r.payload {
            *app_totals.entry(r.app_name.clone()).or_insert(0) += foc.duration_ms;
        }
    }

    // Sort by duration descending
    let mut sorted: Vec<_> = app_totals.into_iter().collect();
    sorted.sort_by_key(|b| std::cmp::Reverse(b.1));

    let mut total_ms = 0u64;
    for (app, duration_ms) in sorted {
        println!(
            "  {:<28} {:>20}",
            &app[..app.len().min(28)],
            fmt_duration_ms(duration_ms)
        );
        total_ms += duration_ms;
    }
    println!("{}", "─".repeat(52));
    println!("  {:<28} {:>20}", "Total", fmt_duration_ms(total_ms));
    println!();
}

/// Format bytes into human-readable units (B, KB, MB, GB).
fn fmt_bytes(b: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;

    match b {
        0..=1023 => format!("{:>3} B", b),
        1024..=1_048_575 => format!("{:>6.1} KB", b as f64 / KB),
        1_048_576..=1_073_741_823 => format!("{:>6.1} MB", b as f64 / MB),
        _ => format!("{:>6.1} GB", b as f64 / GB),
    }
}

/// Format milliseconds into human-readable duration (ms, s, m, h).
fn fmt_duration_ms(ms: u64) -> String {
    const SECOND_MS: u64 = 1_000;
    const MINUTE_MS: u64 = 60_000;
    const HOUR_MS: u64 = 3_600_000;

    match ms {
        0..=999 => format!("{:>4} ms", ms),
        1_000..=59_999 => format!("{:>6.1} s", ms as f64 / SECOND_MS as f64),
        60_000..=3_599_999 => format!("{:>6.1} m", ms as f64 / MINUTE_MS as f64),
        _ => format!("{:>6.2} h", ms as f64 / HOUR_MS as f64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fmt_bytes_ranges() {
        let bytes_0 = fmt_bytes(0);
        assert!(bytes_0.contains("B"), "0 bytes should format with B suffix");

        let bytes_512 = fmt_bytes(512);
        assert!(
            bytes_512.contains("B"),
            "512 bytes should format with B suffix"
        );

        let bytes_1kb = fmt_bytes(1024);
        assert!(
            bytes_1kb.contains("KB"),
            "1024 bytes should format with KB suffix"
        );

        let bytes_1mb = fmt_bytes(1_048_576);
        assert!(bytes_1mb.contains("MB"), "1MB should format with MB suffix");

        let bytes_1gb = fmt_bytes(1_073_741_824);
        assert!(bytes_1gb.contains("GB"), "1GB should format with GB suffix");
    }

    #[test]
    fn test_fmt_duration_ms_ranges() {
        let ms_0 = fmt_duration_ms(0);
        assert!(ms_0.contains("ms"), "0ms should format with ms suffix");

        let ms_500 = fmt_duration_ms(500);
        assert!(ms_500.contains("ms"), "500ms should format with ms suffix");

        let ms_1s = fmt_duration_ms(1000);
        assert!(ms_1s.contains("s"), "1000ms should format with s suffix");

        let ms_1m = fmt_duration_ms(60000);
        assert!(ms_1m.contains("m"), "60000ms should format with m suffix");

        let ms_1h = fmt_duration_ms(3600000);
        assert!(ms_1h.contains("h"), "3600000ms should format with h suffix");
    }

    #[test]
    fn test_print_disk_table_empty() {
        let records: Vec<MetricRecord> = vec![];
        print_snapshot_table(&records, 5);
    }

    #[test]
    fn test_print_disk_table_sorting() {
        use heimwatch_core::DiskData;

        let records = vec![
            MetricRecord {
                app_name: "app_low".to_string(),
                timestamp: 1000,
                payload: MetricPayload::Dsk(DiskData {
                    read_bytes: 100,
                    write_bytes: 100,
                    mount_point: String::new(),
                }),
            },
            MetricRecord {
                app_name: "app_high".to_string(),
                timestamp: 1000,
                payload: MetricPayload::Dsk(DiskData {
                    read_bytes: 1000,
                    write_bytes: 1000,
                    mount_point: String::new(),
                }),
            },
        ];

        print_snapshot_table(&records, 5);
    }

    #[test]
    fn test_print_network_table_empty() {
        let records: Vec<MetricRecord> = vec![];
        print_snapshot_table(&records, 5);
    }

    #[test]
    fn test_print_cpu_table_empty() {
        let records: Vec<MetricRecord> = vec![];
        print_snapshot_table(&records, 5);
    }

    #[test]
    fn test_print_focus_table_empty() {
        let records: Vec<MetricRecord> = vec![];
        print_snapshot_table(&records, 5);
    }
}
