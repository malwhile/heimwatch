//! One-shot metric snapshots: network traffic, CPU usage, and focus time.

use anyhow::{Result, anyhow};
use heimwatch_collector::PlatformCollector;
use heimwatch_core::{MetricPayload, current_unix_timestamp};
use heimwatch_storage::StorageLayer;
use std::time::Duration;

/// Capture a snapshot of metrics (network, CPU, or focus) and print results.
///
/// # Network Snapshot
/// 1. Creates `PlatformCollector::new()` in a blocking task (eBPF FDs are not Send)
/// 2. Waits for the specified window duration (traffic accumulates in BPF map)
/// 3. Calls `collect_network()` once (returns bytes accumulated since step 1)
/// 4. Formats and prints results to stdout
///
/// # CPU Snapshot
/// 1. Creates `PlatformCollector::new()` in a blocking task (eBPF FDs are not Send)
/// 2. Waits for the specified window duration (CPU time accumulates in BPF map)
/// 3. Calls `collect_cpu(window_duration)` once (returns CPU time accumulated since step 1)
/// 4. Formats and prints results to stdout
///
/// # Focus Snapshot
/// 1. Opens the sled database (requires --db argument)
/// 2. Queries focus records from the last N seconds
/// 3. Aggregates focus time by app
/// 4. Formats and prints results to stdout
pub async fn run_snapshot(
    window_secs: u64,
    format: &str,
    metric_type: &str,
    db_path: Option<&str>,
) -> Result<()> {
    match metric_type {
        "cpu" => run_cpu_snapshot(window_secs, format).await,
        "focus" => run_focus_snapshot(window_secs, format, db_path).await,
        "network" => run_network_snapshot(window_secs, format).await,
        _ => anyhow::bail!("Please choose cpu, focus, or network"),
    }
}

/// Capture network traffic snapshot (one-shot probe collection).
async fn run_network_snapshot(window_secs: u64, format: &str) -> Result<()> {
    if window_secs == 0 {
        anyhow::bail!("Window must be greater than 0 seconds");
    }

    log::info!("Attaching eBPF probes, observing for {}s...", window_secs);

    // PlatformCollector::new() blocks on eBPF FD setup
    let mut collector = tokio::task::spawn_blocking(PlatformCollector::new).await??;

    // Extract the network collector
    let mut network_collector = collector
        .take_network_collector()
        .ok_or_else(|| anyhow::anyhow!("Network collection unavailable on this platform"))?;

    // Wait for traffic to accumulate in the BPF map
    tokio::time::sleep(Duration::from_secs(window_secs)).await;

    // Collect: first call returns bytes since probe attachment
    let records =
        tokio::task::spawn_blocking(move || network_collector.collect_network()).await??;

    // Format and output results
    match format {
        "json" => {
            println!("{}", serde_json::to_string_pretty(&records)?);
        }
        _ => {
            print_network_table(&records, window_secs);
        }
    }

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

    // PlatformCollector::new() blocks on eBPF FD setup
    let mut collector = tokio::task::spawn_blocking(PlatformCollector::new).await??;

    // Extract the CPU collector
    let mut cpu_collector = collector
        .take_cpu_collector()
        .ok_or_else(|| anyhow::anyhow!("CPU tracking unavailable on this platform"))?;

    // Wait for CPU time to accumulate in the BPF map
    tokio::time::sleep(Duration::from_secs(window_secs)).await;

    // Collect: first call returns CPU time accumulated since probe attachment
    let records = tokio::task::spawn_blocking(move || {
        cpu_collector.collect_cpu(Duration::from_secs(window_secs))
    })
    .await??;

    // Format and output results
    match format {
        "json" => {
            println!("{}", serde_json::to_string_pretty(&records)?);
        }
        _ => {
            print_cpu_table(&records, window_secs);
        }
    }

    Ok(())
}

/// Capture focus time snapshot (database query).
async fn run_focus_snapshot(window_secs: u64, format: &str, db_path: Option<&str>) -> Result<()> {
    let db_path = db_path.ok_or_else(|| anyhow!("--db argument required for focus snapshot"))?;

    log::info!("Querying focus events from last {}s...", window_secs);

    // Open storage layer
    let storage =
        StorageLayer::open(db_path).map_err(|e| anyhow!("Failed to open database: {}", e))?;

    // Query focus events from the last N seconds
    let now = current_unix_timestamp()?;
    let start = now.saturating_sub(window_secs);
    let end = now;

    let records = storage
        .get_metrics_by_type(heimwatch_core::MetricType::Foc, start, end)
        .map_err(|e| anyhow!("Failed to query focus events: {}", e))?;

    // Format and output results
    match format {
        "json" => {
            println!("{}", serde_json::to_string_pretty(&records)?);
        }
        _ => {
            print_focus_table(&records, window_secs);
        }
    }

    Ok(())
}

/// Print a human-readable table of network traffic by application.
fn print_network_table(records: &[heimwatch_core::MetricRecord], window_secs: u64) {
    println!("\nHeiwatch Network Snapshot ({}s window)", window_secs);
    println!("{}", "─".repeat(52));
    println!("  {:<28} {:>10} {:>10}", "App", "TX", "RX");
    println!("  {}", "─".repeat(48));

    if records.is_empty() {
        println!("  (no traffic observed)");
    } else {
        let (mut total_tx, mut total_rx) = (0u64, 0u64);
        for r in records {
            if let MetricPayload::Net(net) = &r.payload {
                println!(
                    "  {:<28} {:>10} {:>10}",
                    &r.app_name[..r.app_name.len().min(28)], // truncate long names
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
    }
    println!();
}

/// Print a human-readable table of CPU usage by application.
fn print_cpu_table(records: &[heimwatch_core::MetricRecord], window_secs: u64) {
    println!("\nHeiwatch CPU Usage Snapshot ({}s window)", window_secs);
    println!("{}", "─".repeat(52));
    println!("  {:<28} {:>20}", "App", "CPU Usage");
    println!("  {}", "─".repeat(48));

    if records.is_empty() {
        println!("  (no CPU activity observed)");
    } else {
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
    }
    println!();
}

/// Print a human-readable table of focus time by application.
fn print_focus_table(records: &[heimwatch_core::MetricRecord], window_secs: u64) {
    println!("\nHeiwatch Focus Time Snapshot ({}s window)", window_secs);
    println!("{}", "─".repeat(52));
    println!("  {:<28} {:>20}", "App", "Focus Time");
    println!("  {}", "─".repeat(48));

    if records.is_empty() {
        println!("  (no focus events recorded)");
    } else {
        // Aggregate focus time by app
        let mut app_totals: std::collections::HashMap<String, u64> =
            std::collections::HashMap::new();
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
    }
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
