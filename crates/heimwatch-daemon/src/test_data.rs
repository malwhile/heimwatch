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

            // Focus data
            let foc_record = MetricRecord {
                app_name: app_name.to_string(),
                timestamp,
                payload: MetricPayload::Foc(FocusData {
                    app_id: app_name.to_string(),
                    duration_ms: rng.gen_range(1000..300000),
                }),
            };
            storage.insert_metric(&foc_record)?;

            // CPU data
            let cpu_record = MetricRecord {
                app_name: app_name.to_string(),
                timestamp,
                payload: MetricPayload::Cpu(CpuData {
                    cpu_time_ns: rng.gen_range(100_000_000..2_000_000_000),
                    cpu_usage_percent: rng.gen_range(0.5..95.0),
                }),
            };
            storage.insert_metric(&cpu_record)?;

            // Memory data
            let mem_record = MetricRecord {
                app_name: app_name.to_string(),
                timestamp,
                payload: MetricPayload::Mem(MemoryData {
                    rss_bytes: rng.gen_range(50_000_000..2_000_000_000),
                    vms_bytes: rng.gen_range(100_000_000..4_000_000_000),
                    swap_bytes: rng.gen_range(0..500_000_000),
                    process_count: rng.gen_range(1..10),
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

    println!(
        "Generated {} records for {} apps spanning {} hours",
        records_per_app * TEST_APPS.len() as u32 * 8,
        TEST_APPS.len(),
        hours_back
    );

    Ok(())
}
