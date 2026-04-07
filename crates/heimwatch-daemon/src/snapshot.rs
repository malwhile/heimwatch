//! One-shot network traffic snapshot: attach eBPF probes, observe for N seconds, print results.

use anyhow::Result;
use heimwatch_collector::PlatformCollector;
use heimwatch_core::{Collector, MetricPayload};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Capture a one-shot snapshot of network traffic over a window period.
///
/// # Design
/// 1. Creates `PlatformCollector::new()` in a blocking task (eBPF FDs are not Send)
/// 2. Waits for the specified window duration (traffic accumulates in BPF map)
/// 3. Calls `collect_network()` once (returns bytes accumulated since step 1)
/// 4. Formats and prints results to stdout
///
/// The first call to `collect_network()` returns deltas against an empty `prev_state`,
/// which effectively gives us absolute bytes accumulated during the window period.
pub async fn run_snapshot(window_secs: u64, format: &str) -> Result<()> {
    log::info!("Attaching eBPF probes, observing for {}s...", window_secs);

    // PlatformCollector::new() blocks on eBPF FD setup
    let collector = tokio::task::spawn_blocking(PlatformCollector::new).await??;
    let collector = Arc::new(Mutex::new(collector));

    // Wait for traffic to accumulate in the BPF map
    tokio::time::sleep(Duration::from_secs(window_secs)).await;

    // Collect: first call returns bytes since probe attachment
    let records = {
        let col = Arc::clone(&collector);
        tokio::task::spawn_blocking(move || {
            let mut collector = col.lock().unwrap_or_else(|p| p.into_inner());
            collector.collect_network()
        })
        .await??
    };

    // Format and output results
    match format {
        "json" => {
            println!("{}", serde_json::to_string_pretty(&records)?);
        }
        _ => {
            print_text_table(&records, window_secs);
        }
    }

    Ok(())
}

/// Print a human-readable table of network traffic by application.
fn print_text_table(records: &[heimwatch_core::MetricRecord], window_secs: u64) {
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
