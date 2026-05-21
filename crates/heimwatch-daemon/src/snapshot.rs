//! One-shot metric snapshots: network traffic, CPU usage, disk I/O, memory usage, focus time, and power.

use anyhow::{Result, anyhow};
use heimwatch_core::{MetricPayload, MetricRecord, PowerData, current_unix_timestamp};
use heimwatch_storage::{AppPowerStats, StorageLayer};
use std::collections::HashMap;
use std::time::Duration;

// Separator widths for table formatting
const SEPARATOR_WIDTH_NETWORK: usize = 52;
const SEPARATOR_WIDTH_CPU: usize = 70;
const SEPARATOR_WIDTH_DISK: usize = 64;
const SEPARATOR_WIDTH_MEMORY: usize = 80;
const SEPARATOR_WIDTH_FOCUS: usize = 52;
const SEPARATOR_WIDTH_GPU: usize = 100;
const SEPARATOR_WIDTH_GPU_PROC: usize = 68;

/// Truncate app name for display, preserving UTF-8 safety.
fn format_app_name(name: &str, max_len: usize) -> &str {
    &name[..name.len().min(max_len)]
}

/// Format an optional percentage value with "N/A" fallback and right-alignment.
fn format_optional_percentage(val: Option<f32>) -> String {
    val.map(|v| format!("{:>6.1}%", v))
        .unwrap_or_else(|| "   N/A".to_string())
}

/// Capture a snapshot of metrics (network, CPU, disk, memory, GPU, or focus) and print results.
///
/// Dispatches to platform-specific collectors or database queries based on metric_type.
pub async fn run_snapshot(
    window_secs: u64,
    format: &str,
    metric_type: &str,
    db_path: Option<&str>,
) -> Result<()> {
    if window_secs == 0 {
        anyhow::bail!("Window must be greater than 0 seconds");
    }

    match metric_type {
        "cpu" => run_cpu_snapshot(window_secs, format).await,
        "disk" => run_disk_snapshot(window_secs, format).await,
        "focus" => run_focus_snapshot(window_secs, format, db_path).await,
        "gpu" => run_gpu_snapshot(window_secs, format).await,
        "memory" => run_memory_snapshot(window_secs, format).await,
        "network" => run_network_snapshot(window_secs, format).await,
        _ => anyhow::bail!("Please choose cpu, disk, focus, gpu, memory, or network"),
    }
}

/// Capture network traffic snapshot (one-shot probe collection).
async fn run_network_snapshot(window_secs: u64, format: &str) -> Result<()> {
    log::info!("Attaching eBPF probes, observing for {}s...", window_secs);
    let mut network_collector =
        tokio::task::spawn_blocking(heimwatch_collector::NetworkCollector::new)
            .await?
            .map_err(|e| anyhow!("Network collection unavailable on this platform: {}", e))?;

    tokio::time::sleep(Duration::from_secs(window_secs)).await;
    let records =
        tokio::task::spawn_blocking(move || network_collector.collect_network()).await??;

    format_output(format, &records, window_secs)?;
    Ok(())
}

/// Capture CPU usage snapshot (one-shot probe collection).
async fn run_cpu_snapshot(window_secs: u64, format: &str) -> Result<()> {
    log::info!(
        "Attaching eBPF sched_switch probe, observing for {}s...",
        window_secs
    );
    let mut cpu_collector = tokio::task::spawn_blocking(heimwatch_collector::CpuCollector::new)
        .await?
        .map_err(|e| anyhow!("CPU tracking unavailable on this platform: {}", e))?;

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
    let mut disk_collector = tokio::task::spawn_blocking(heimwatch_collector::DiskCollector::new)
        .await?
        .map_err(|e| anyhow!("Disk I/O tracking unavailable on this platform: {}", e))?;

    tokio::time::sleep(Duration::from_secs(window_secs)).await;
    let records = tokio::task::spawn_blocking(move || {
        disk_collector.collect_disk(Duration::from_secs(window_secs))
    })
    .await??;

    format_output(format, &records, window_secs)?;
    Ok(())
}

/// Capture memory usage snapshot (polling collection).
async fn run_memory_snapshot(window_secs: u64, format: &str) -> Result<()> {
    log::info!(
        "Polling /proc for memory usage, observing for {}s...",
        window_secs
    );
    let memory_collector = tokio::task::spawn_blocking(heimwatch_collector::MemoryCollector::new)
        .await?
        .map_err(|e| anyhow!("Memory tracking unavailable on this platform: {}", e))?;

    tokio::time::sleep(Duration::from_secs(window_secs)).await;
    let records = tokio::task::spawn_blocking(move || memory_collector.collect_memory()).await??;

    format_output(format, &records, window_secs)?;
    Ok(())
}

/// Capture GPU metrics snapshot (polling collection).
async fn run_gpu_snapshot(window_secs: u64, format: &str) -> Result<()> {
    log::info!("Polling GPU metrics, observing for {}s...", window_secs);
    let mut gpu_collector = tokio::task::spawn_blocking(heimwatch_collector::GpuCollector::new)
        .await?
        .map_err(|e| anyhow!("GPU metrics unavailable on this platform: {}", e))?;

    // Baseline poll (populates prev_fdinfo for per-process delta calculation)
    let _ = gpu_collector.collect_gpus(Duration::from_secs(1))?;

    tokio::time::sleep(Duration::from_secs(window_secs)).await;
    let records = gpu_collector.collect_gpus(Duration::from_secs(window_secs))?;

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

/// Capture power usage snapshot (database query).
pub async fn run_power_snapshot(
    window_secs: u64,
    format: &str,
    db_path: &str,
    limit: usize,
) -> Result<()> {
    log::info!("Querying power usage from last {}s...", window_secs);

    let storage =
        StorageLayer::open(db_path).map_err(|e| anyhow!("Failed to open database: {}", e))?;

    let now = current_unix_timestamp()?;
    let start = now.saturating_sub(window_secs);
    let end = now;

    let plugged_in = storage
        .get_top_apps_by_power(start, end, Some(false), limit)
        .map_err(|e| anyhow!("Failed to query power stats (plugged in): {}", e))?;

    let on_battery = storage
        .get_top_apps_by_power(start, end, Some(true), limit)
        .map_err(|e| anyhow!("Failed to query power stats (on battery): {}", e))?;

    let system_power = storage
        .get_system_power_history(start, end)
        .map_err(|e| anyhow!("Failed to query system power history: {}", e))?;

    match format {
        "json" => {
            let result = serde_json::json!({
                "window_secs": window_secs,
                "plugged_in": plugged_in,
                "on_battery": on_battery,
                "system_power": system_power,
            });
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        _ => {
            print_power_table(&plugged_in, &on_battery, &system_power, window_secs);
        }
    }
    Ok(())
}

/// Print power statistics in human-readable table format.
fn print_power_table(
    plugged_in: &[AppPowerStats],
    on_battery: &[AppPowerStats],
    system_power: &[MetricRecord],
    window_secs: u64,
) {
    println!("\nPower Usage (last {}s)\n", window_secs);

    print_power_section("Plugged In", plugged_in);
    print_power_section("On Battery", on_battery);

    if let Some(latest) = system_power.last()
        && let MetricPayload::Pwr(pwr) = &latest.payload
    {
        print_system_power_summary(pwr);
    }
}

/// Print a single power section (Plugged In or On Battery).
fn print_power_section(label: &str, stats: &[AppPowerStats]) {
    println!("Top Apps by Power Percentage ({}):", label);
    if stats.is_empty() {
        let state = label.to_lowercase();
        println!("  (no data: not in {} state during this window)\n", state);
        return;
    }

    for (i, stat) in stats.iter().enumerate() {
        println!(
            "  {:>2}. {:<20} {:>5.1}%  (CPU: {:>2.0}%, GPU: {:>2.0}%, Disp: {:>2.0}%, Disk: {:>2.0}%, Net: {:>2.0}%, Mem: {:>2.0}%)",
            i + 1,
            format_app_name(&stat.app_name, 20),
            stat.power_pct,
            stat.cpu_contribution * 100.0,
            stat.gpu_contribution * 100.0,
            stat.display_contribution * 100.0,
            stat.disk_contribution * 100.0,
            stat.net_contribution * 100.0,
            stat.mem_contribution * 100.0,
        );
    }
    println!();
}

/// Print system-level power summary (battery, charging, RAPL).
fn print_system_power_summary(pwr: &PowerData) {
    println!("System Power (latest):");
    match pwr.battery_percent {
        Some(pct) => println!("  Battery: {:.0}%", pct),
        None => println!("  Battery: N/A"),
    }
    println!("  Charging: {}", pwr.charging);
    match pwr.rapl_package_watts {
        Some(w) => println!("  RAPL Package: {:.1}W", w),
        None => println!("  RAPL Package: N/A"),
    }
    println!();
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
        MetricPayload::Mem(_) => print_memory_table(records, window_secs),
        MetricPayload::Foc(_) => print_focus_table(records, window_secs),
        MetricPayload::Gpu(_) => {
            // Separate aggregate and per-process GPU records
            let gpu_records: Vec<_> = records
                .iter()
                .filter(|r| matches!(r.payload, MetricPayload::Gpu(_)))
                .collect();
            let gpu_proc_records: Vec<_> = records
                .iter()
                .filter(|r| matches!(r.payload, MetricPayload::GpuProc(_)))
                .collect();

            if !gpu_records.is_empty() {
                print_gpu_table(
                    &gpu_records.iter().map(|r| (*r).clone()).collect::<Vec<_>>(),
                    window_secs,
                );
            }
            if !gpu_proc_records.is_empty() {
                print_gpu_proc_table(
                    &gpu_proc_records
                        .iter()
                        .map(|r| (*r).clone())
                        .collect::<Vec<_>>(),
                    window_secs,
                );
            }
        }
        MetricPayload::GpuProc(_) => {
            // Handle per-process GPU records
            print_gpu_proc_table(records, window_secs);
        }
        _ => println!("  (unsupported metric type)"),
    }
}

/// Print a human-readable table of network traffic by application.
fn print_network_table(records: &[MetricRecord], window_secs: u64) {
    println!("\nHeiwatch Network Snapshot ({}s window)", window_secs);
    println!("{}", "─".repeat(SEPARATOR_WIDTH_NETWORK));
    println!("  {:<28} {:>10} {:>10}", "App", "TX", "RX");
    println!("  {}", "─".repeat(SEPARATOR_WIDTH_NETWORK - 4));

    let (mut total_tx, mut total_rx) = (0u64, 0u64);
    for r in records {
        if let MetricPayload::Net(net) = &r.payload {
            println!(
                "  {:<28} {:>10} {:>10}",
                format_app_name(&r.app_name, 28),
                fmt_bytes(net.tx_bytes),
                fmt_bytes(net.rx_bytes)
            );
            total_tx += net.tx_bytes;
            total_rx += net.rx_bytes;
        }
    }
    println!("{}", "─".repeat(SEPARATOR_WIDTH_NETWORK));
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
    println!("\nHeiwatch CPU Snapshot ({}s window)", window_secs);
    println!("{}", "─".repeat(SEPARATOR_WIDTH_CPU));
    println!("  {:<28} {:>18} {:>18}", "App", "CPU Time", "Usage %");
    println!("  {}", "─".repeat(SEPARATOR_WIDTH_CPU - 4));

    // Sort by CPU time descending
    let mut sorted: Vec<_> = records.iter().collect();
    sorted.sort_by(|a, b| {
        let time_a = if let MetricPayload::Cpu(cpu) = &a.payload {
            cpu.cpu_time_ns
        } else {
            0
        };
        let time_b = if let MetricPayload::Cpu(cpu) = &b.payload {
            cpu.cpu_time_ns
        } else {
            0
        };
        time_b.cmp(&time_a)
    });

    let mut total_time_ns = 0u64;
    let mut total_usage_percent = 0.0f32;
    for r in sorted {
        if let MetricPayload::Cpu(cpu) = &r.payload {
            println!(
                "  {:<28} {:>18} {:>17.1}%",
                format_app_name(&r.app_name, 28),
                fmt_cpu_time_ns(cpu.cpu_time_ns),
                cpu.cpu_usage_percent
            );
            total_time_ns += cpu.cpu_time_ns;
            total_usage_percent += cpu.cpu_usage_percent;
        }
    }
    println!("{}", "─".repeat(SEPARATOR_WIDTH_CPU));
    println!(
        "  {:<28} {:>18} {:>17.1}%",
        "Total",
        fmt_cpu_time_ns(total_time_ns),
        total_usage_percent
    );
    println!();
}

/// Print a human-readable table of disk I/O by application.
fn print_disk_table(records: &[MetricRecord], window_secs: u64) {
    println!("\nHeiwatch Disk I/O Snapshot ({}s window)", window_secs);
    println!("{}", "─".repeat(SEPARATOR_WIDTH_DISK));
    println!("  {:<28} {:>10} {:>10}", "App", "Read", "Write");
    println!("  {}", "─".repeat(SEPARATOR_WIDTH_DISK - 4));

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
                format_app_name(&r.app_name, 28),
                fmt_bytes(dsk.read_bytes),
                fmt_bytes(dsk.write_bytes)
            );
            total_read += dsk.read_bytes;
            total_write += dsk.write_bytes;
        }
    }
    println!("{}", "─".repeat(SEPARATOR_WIDTH_DISK));
    println!(
        "  {:<28} {:>10} {:>10}",
        "Total",
        fmt_bytes(total_read),
        fmt_bytes(total_write)
    );
    println!();
}

/// Print a human-readable table of memory usage by application.
fn print_memory_table(records: &[MetricRecord], window_secs: u64) {
    println!("\nHeiwatch Memory Usage Snapshot ({}s window)", window_secs);
    println!("{}", "─".repeat(SEPARATOR_WIDTH_MEMORY));
    println!(
        "  {:<28} {:>10} {:>10} {:>10} {:>10}",
        "App", "RSS", "VMS", "Swap", "Procs"
    );
    println!("  {}", "─".repeat(SEPARATOR_WIDTH_MEMORY - 4));

    // Sort by RSS descending
    let mut sorted: Vec<_> = records.iter().collect();
    sorted.sort_by(|a, b| {
        let rss_a = if let MetricPayload::Mem(mem) = &a.payload {
            mem.rss_bytes
        } else {
            0
        };
        let rss_b = if let MetricPayload::Mem(mem) = &b.payload {
            mem.rss_bytes
        } else {
            0
        };
        rss_b.cmp(&rss_a)
    });

    let (mut total_rss, mut total_vms, mut total_swap) = (0u64, 0u64, 0u64);
    for r in sorted {
        if let MetricPayload::Mem(mem) = &r.payload {
            println!(
                "  {:<28} {:>10} {:>10} {:>10} {:>10}",
                format_app_name(&r.app_name, 28),
                fmt_bytes(mem.rss_bytes),
                fmt_bytes(mem.vms_bytes),
                fmt_bytes(mem.swap_bytes),
                mem.process_count
            );
            total_rss += mem.rss_bytes;
            total_vms += mem.vms_bytes;
            total_swap += mem.swap_bytes;
        }
    }
    println!("{}", "─".repeat(SEPARATOR_WIDTH_MEMORY));
    println!(
        "  {:<28} {:>10} {:>10} {:>10}",
        "Total",
        fmt_bytes(total_rss),
        fmt_bytes(total_vms),
        fmt_bytes(total_swap)
    );
    println!();
}

/// Print a human-readable table of focus time by application.
fn print_focus_table(records: &[MetricRecord], window_secs: u64) {
    println!("\nHeiwatch Focus Time Snapshot ({}s window)", window_secs);
    println!("{}", "─".repeat(SEPARATOR_WIDTH_FOCUS));
    println!("  {:<28} {:>20}", "App", "Focus Time");
    println!("  {}", "─".repeat(SEPARATOR_WIDTH_FOCUS - 4));

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
            format_app_name(&app, 28),
            fmt_duration_ms(duration_ms)
        );
        total_ms += duration_ms;
    }
    println!("{}", "─".repeat(SEPARATOR_WIDTH_FOCUS));
    println!("  {:<28} {:>20}", "Total", fmt_duration_ms(total_ms));
    println!();
}

/// Print a human-readable table of GPU metrics.
fn print_gpu_table(records: &[MetricRecord], window_secs: u64) {
    println!("\nHeiwatch GPU Snapshot ({}s window)", window_secs);
    println!("{}", "─".repeat(SEPARATOR_WIDTH_GPU));
    println!(
        "  {:<28} {:<8} {:<12} {:<10} {:<10} {:<14}",
        "GPU", "Vendor", "Usage", "VRAM", "Temp", "Clock"
    );
    println!("  {}", "─".repeat(SEPARATOR_WIDTH_GPU - 4));

    for r in records {
        if let MetricPayload::Gpu(gpu) = &r.payload {
            let vendor_str = match gpu.vendor {
                heimwatch_core::GpuVendor::Nvidia => "NVIDIA",
                heimwatch_core::GpuVendor::Amd => "AMD",
                heimwatch_core::GpuVendor::Intel => "Intel",
                heimwatch_core::GpuVendor::Unknown => "Unknown",
            };

            let usage_str = format_optional_percentage(gpu.usage_percent);

            let vram_str = match (gpu.vram_used_bytes, gpu.vram_total_bytes) {
                (Some(used), Some(total)) => {
                    format!("{}/{}", fmt_bytes(used), fmt_bytes(total))
                }
                _ => "N/A".to_string(),
            };

            let temp_str = gpu
                .temperature_celsius
                .map(|t| format!("{:>7.1}°C", t))
                .unwrap_or_else(|| "  N/A".to_string());

            let clock_str = gpu
                .core_clock_mhz
                .map(|c| format!("{:>6} MHz", c))
                .unwrap_or_else(|| "  N/A".to_string());

            println!(
                "  {:<28} {:<8} {:>12} {:<10} {:<10} {:<14}",
                format_app_name(&gpu.name, 28),
                vendor_str,
                usage_str,
                vram_str,
                temp_str,
                clock_str
            );
        }
    }
    println!("{}", "─".repeat(SEPARATOR_WIDTH_GPU));
    println!();
}

/// Print a human-readable table of per-process GPU usage by application.
fn print_gpu_proc_table(records: &[MetricRecord], window_secs: u64) {
    println!(
        "\nHeiwatch GPU Per-Process Snapshot ({}s window)",
        window_secs
    );
    println!("{}", "─".repeat(SEPARATOR_WIDTH_GPU_PROC));
    println!(
        "  {:<28} {:<6} {:>12} {:>12}",
        "App", "GPU", "Usage", "VRAM"
    );
    println!("  {}", "─".repeat(SEPARATOR_WIDTH_GPU_PROC - 4));

    // Sort by usage descending
    let mut sorted: Vec<_> = records.iter().collect();
    sorted.sort_by(|a, b| {
        let usage_a = if let MetricPayload::GpuProc(proc) = &a.payload {
            proc.usage_percent.unwrap_or(0.0)
        } else {
            0.0
        };
        let usage_b = if let MetricPayload::GpuProc(proc) = &b.payload {
            proc.usage_percent.unwrap_or(0.0)
        } else {
            0.0
        };
        usage_b
            .partial_cmp(&usage_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    for r in sorted {
        if let MetricPayload::GpuProc(proc) = &r.payload {
            let usage_str = proc
                .usage_percent
                .map(|u| format!("{:>10.1}%", u))
                .unwrap_or_else(|| "       N/A".to_string());

            let vram_str = proc
                .vram_used_bytes
                .map(fmt_bytes)
                .unwrap_or_else(|| "N/A".to_string());

            println!(
                "  {:<28} {:<6} {:<12} {:>12}",
                format_app_name(&r.app_name, 28),
                proc.gpu_index,
                usage_str,
                vram_str
            );
        }
    }
    println!("{}", "─".repeat(SEPARATOR_WIDTH_GPU_PROC));
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

/// Format nanoseconds into human-readable duration (us, ms, s, m, h).
fn fmt_cpu_time_ns(ns: u64) -> String {
    const US_PER_NS: f64 = 1.0 / 1_000.0;
    const MS_PER_NS: f64 = 1.0 / 1_000_000.0;
    const S_PER_NS: f64 = 1.0 / 1_000_000_000.0;

    match ns {
        0..=999_999 => format!("{:>6.0} us", ns as f64 * US_PER_NS),
        1_000_000..=999_999_999 => format!("{:>6.1} ms", ns as f64 * MS_PER_NS),
        1_000_000_000..=59_999_999_999 => {
            let secs = ns as f64 * S_PER_NS;
            format!("{:>6.1} s", secs)
        }
        60_000_000_000..=3_599_999_999_999 => {
            let secs = ns as f64 * S_PER_NS;
            let mins = secs / 60.0;
            format!("{:>6.1} m", mins)
        }
        _ => {
            let secs = ns as f64 * S_PER_NS;
            let hours = secs / 3600.0;
            format!("{:>6.2} h", hours)
        }
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

    #[test]
    fn test_print_gpu_table_empty() {
        let records: Vec<MetricRecord> = vec![];
        print_snapshot_table(&records, 5);
    }

    #[test]
    fn test_print_gpu_table_with_data() {
        use heimwatch_core::{GpuData, GpuVendor};

        let records = vec![
            MetricRecord {
                app_name: "gpu:0".to_string(),
                timestamp: 1000,
                payload: MetricPayload::Gpu(GpuData {
                    gpu_index: 0,
                    vendor: GpuVendor::Nvidia,
                    name: "NVIDIA GeForce RTX 3090".to_string(),
                    usage_percent: Some(75.5),
                    vram_used_bytes: Some(8 * 1024 * 1024 * 1024),
                    vram_total_bytes: Some(24 * 1024 * 1024 * 1024),
                    temperature_celsius: Some(65.0),
                    power_draw_watts: Some(350.0),
                    core_clock_mhz: Some(2400),
                    memory_clock_mhz: Some(9000),
                }),
            },
            MetricRecord {
                app_name: "gpu:1".to_string(),
                timestamp: 1000,
                payload: MetricPayload::Gpu(GpuData {
                    gpu_index: 1,
                    vendor: GpuVendor::Amd,
                    name: "AMD Radeon RX 6900 XT".to_string(),
                    usage_percent: Some(50.0),
                    vram_used_bytes: Some(4 * 1024 * 1024 * 1024),
                    vram_total_bytes: Some(16 * 1024 * 1024 * 1024),
                    temperature_celsius: Some(60.0),
                    power_draw_watts: Some(250.0),
                    core_clock_mhz: Some(2100),
                    memory_clock_mhz: Some(8000),
                }),
            },
        ];

        print_snapshot_table(&records, 5);
    }

    #[test]
    fn test_print_power_section_empty() {
        let stats: Vec<AppPowerStats> = vec![];
        print_power_section("Plugged In", &stats);
    }

    #[test]
    fn test_print_power_section_with_data() {
        let stats = vec![
            AppPowerStats {
                app_name: "Firefox".to_string(),
                power_pct: 35.2,
                power_score: 10.5,
                on_battery: false,
                cpu_contribution: 0.25,
                gpu_contribution: 0.0,
                display_contribution: 0.10,
                disk_contribution: 0.08,
                net_contribution: 0.02,
                mem_contribution: 0.05,
            },
            AppPowerStats {
                app_name: "VS Code".to_string(),
                power_pct: 22.1,
                power_score: 6.6,
                on_battery: false,
                cpu_contribution: 0.18,
                gpu_contribution: 0.0,
                display_contribution: 0.0,
                disk_contribution: 0.03,
                net_contribution: 0.01,
                mem_contribution: 0.0,
            },
        ];
        print_power_section("Plugged In", &stats);
    }

    #[test]
    fn test_print_system_power_summary_with_battery() {
        let pwr = PowerData {
            watt_usage: 18.2,
            battery_percent: Some(75.0),
            charging: false,
            rapl_package_watts: Some(18.2),
            rapl_core_watts: None,
            battery_current_ua: None,
            battery_voltage_uv: None,
        };
        print_system_power_summary(&pwr);
    }

    #[test]
    fn test_print_system_power_summary_no_battery() {
        let pwr = PowerData {
            watt_usage: 0.0,
            battery_percent: None,
            charging: false,
            rapl_package_watts: None,
            rapl_core_watts: None,
            battery_current_ua: None,
            battery_voltage_uv: None,
        };
        print_system_power_summary(&pwr);
    }
}
