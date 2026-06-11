//! Generate realistic test data for manual TUI testing.

use anyhow::Result;
use heimwatch_core::{
    CpuData, DiskData, FocusData, GpuData, GpuProcessData, GpuVendor, MemoryData, MetricPayload,
    MetricRecord, NetworkData, PowerData,
};
use heimwatch_storage::StorageLayer;
use rand::Rng;
use std::time::{SystemTime, UNIX_EPOCH};

const TEST_APPS: &[&str] = &[
    "firefox", "vscode", "spotify", "discord", "slack", "chrome", "terminal", "system",
];

pub fn generate_test_data(db_path: &str, hours_back: u32, records_per_app: u32) -> Result<()> {
    let storage = StorageLayer::open(db_path)?;
    let mut rng = rand::thread_rng();

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let interval_seconds = (hours_back as u64 * 3600) / records_per_app as u64;

    for test_app in TEST_APPS {
        let app_name = *test_app;

        for record_idx in 0..records_per_app {
            let timestamp =
                now - (hours_back as u64 * 3600) + (record_idx as u64 * interval_seconds);

            // Network data
            let net_record = MetricRecord {
                app_name: app_name.to_string(),
                timestamp,
                payload: MetricPayload::Net(NetworkData {
                    tx_bytes: rng.gen_range(100_000..10_000_000),
                    rx_bytes: rng.gen_range(500_000..50_000_000),
                    connections: rng.gen_range(1..20),
                }),
            };
            storage.insert_metric(&net_record)?;

            // Power data
            let pwr_record = MetricRecord {
                app_name: app_name.to_string(),
                timestamp,
                payload: MetricPayload::Pwr(PowerData {
                    watt_usage: rng.gen_range(0.5..15.0),
                    battery_percent: Some(rng.gen_range(20.0..100.0)),
                    charging: record_idx % 3 == 0,
                    rapl_package_watts: Some(rng.gen_range(10.0..60.0)),
                    rapl_core_watts: Some(rng.gen_range(5.0..40.0)),
                    battery_current_ua: Some(rng.gen_range(-2000000..100000)),
                    battery_voltage_uv: Some(rng.gen_range(11000000..13000000)),
                    avg_cpu_freq_ratio: Some(rng.gen_range(0.5..1.0)),
                    display_brightness: Some(rng.gen_range(0.3..1.0)),
                    is_wifi: Some(rng.gen_bool(0.5)),
                }),
            };
            storage.insert_metric(&pwr_record)?;

            // Focus data - higher focus time to make display contribution visible
            // Each record represents time the app was in focus during that interval
            let focus_duration_ms = if rng.gen_bool(0.6) {
                // 60% of the time: app is focused (2-5 minutes per interval)
                rng.gen_range(120_000..300_000)
            } else {
                // 40% of the time: app not focused
                0
            };

            let foc_record = MetricRecord {
                app_name: app_name.to_string(),
                timestamp,
                payload: MetricPayload::Foc(FocusData {
                    app_id: app_name.to_string(),
                    duration_ms: focus_duration_ms,
                }),
            };
            storage.insert_metric(&foc_record)?;

            // CPU data - realistic usage (most apps idle or low usage)
            let cpu_usage_percent = if rng.gen_bool(0.7) {
                // 70% of the time: light usage (0-2%)
                rng.gen_range(0.0..2.0)
            } else if rng.gen_bool(0.8) {
                // 20% of the time: moderate usage (2-8%)
                rng.gen_range(2.0..8.0)
            } else {
                // 10% of the time: higher usage (8-25%)
                rng.gen_range(8.0..25.0)
            };

            let thread_count = if cpu_usage_percent > 20.0 {
                rng.gen_range(4..16) // Heavy users: more threads
            } else if cpu_usage_percent > 5.0 {
                rng.gen_range(2..8) // Moderate: some threads
            } else {
                rng.gen_range(1..4) // Light: few threads
            };

            let cpu_record = MetricRecord {
                app_name: app_name.to_string(),
                timestamp,
                payload: MetricPayload::Cpu(CpuData {
                    // cpu_time_ns = (cpu_usage_percent / 100.0) * interval_ns * num_cores
                    // Assuming 8 cores and 1-second interval for this example
                    cpu_time_ns: (cpu_usage_percent / 100.0 * 1_000_000_000.0 * 8.0) as u64,
                    cpu_usage_percent,
                    thread_count,
                }),
            };
            storage.insert_metric(&cpu_record)?;

            // Memory data - realistic usage
            let rss_bytes = if rng.gen_bool(0.3) {
                // 30%: light processes (50-200 MB)
                rng.gen_range(50_000_000..200_000_000)
            } else if rng.gen_bool(0.6) {
                // 40%: moderate processes (200-600 MB)
                rng.gen_range(200_000_000..600_000_000)
            } else {
                // 30%: heavier processes (600MB-1.5GB)
                rng.gen_range(600_000_000..1_500_000_000)
            };

            let mem_record = MetricRecord {
                app_name: app_name.to_string(),
                timestamp,
                payload: MetricPayload::Mem(MemoryData {
                    rss_bytes,
                    vms_bytes: rss_bytes + rng.gen_range(100_000_000..500_000_000),
                    swap_bytes: rng.gen_range(0..100_000_000), // Minimal swap usage
                    process_count: rng.gen_range(1..5),
                }),
            };
            storage.insert_metric(&mem_record)?;

            // Disk data
            let dsk_record = MetricRecord {
                app_name: app_name.to_string(),
                timestamp,
                payload: MetricPayload::Dsk(DiskData {
                    read_bytes: rng.gen_range(1_000_000..500_000_000),
                    write_bytes: rng.gen_range(500_000..200_000_000),
                    mount_point: "/".to_string(),
                }),
            };
            storage.insert_metric(&dsk_record)?;

            // GPU data
            let gpu_record = MetricRecord {
                app_name: app_name.to_string(),
                timestamp,
                payload: MetricPayload::Gpu(GpuData {
                    gpu_index: 0,
                    vendor: GpuVendor::Nvidia,
                    name: "NVIDIA GeForce RTX 4090".to_string(),
                    usage_percent: if record_idx % 2 == 0 {
                        Some(rng.gen_range(0.0..100.0))
                    } else {
                        None
                    },
                    vram_used_bytes: Some(rng.gen_range(1_000_000_000..22_000_000_000)),
                    vram_total_bytes: Some(24_000_000_000),
                    temperature_celsius: Some(rng.gen_range(30.0..85.0)),
                    power_draw_watts: Some(rng.gen_range(50.0..450.0)),
                    core_clock_mhz: Some(rng.gen_range(1000..2500)),
                    memory_clock_mhz: Some(rng.gen_range(9000..21000)),
                }),
            };
            storage.insert_metric(&gpu_record)?;

            // GPU process data
            let gpu_proc_record = MetricRecord {
                app_name: app_name.to_string(),
                timestamp,
                payload: MetricPayload::GpuProc(GpuProcessData {
                    gpu_index: 0,
                    usage_percent: if record_idx % 3 == 0 {
                        Some(rng.gen_range(0.0..100.0))
                    } else {
                        None
                    },
                    vram_used_bytes: Some(rng.gen_range(100_000_000..4_000_000_000)),
                }),
            };
            storage.insert_metric(&gpu_proc_record)?;
        }
    }

    // Explicitly flush to ensure all data is written to disk
    storage.flush()?;

    println!(
        "✓ Generated {} records for {} apps spanning {} hours",
        records_per_app * TEST_APPS.len() as u32 * 8,
        TEST_APPS.len(),
        hours_back
    );
    println!();
    println!("To view the data in the TUI:");
    println!(
        "  1. Run: cargo run -p heimwatch-daemon -- tui --db {}",
        db_path
    );
    println!(
        "  2. Press '-' to shrink the time window to {} hours",
        hours_back
    );
    println!(
        "     (TUI defaults to 24 hours, but data spans only {} hours)",
        hours_back
    );
    println!("  3. Navigate tabs with ← / → arrow keys to explore");
    println!("  4. Power tab will show realistic Display contribution after window adjustment");

    Ok(())
}
