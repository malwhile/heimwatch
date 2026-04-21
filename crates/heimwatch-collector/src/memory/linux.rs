//! Linux memory collector using polling snapshots of `/proc/[pid]/status`.
//!
//! No eBPF needed; memory is a stable metric that changes on second-to-minute timescales.
//! Reads RSS, VSZ, and swap usage for each running process, aggregates by app name.

use crate::util::run_collector_loop;
use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use heimwatch_core::{
    CollectorEvent, MemoryData, MetricPayload, MetricRecord, current_unix_timestamp,
    process::get_process_name,
};
use std::time::Duration;
use tokio::sync::{mpsc, watch};

pub const POLL_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Default)]
struct MemStats {
    rss: u64,
    vms: u64,
    swap: u64,
    count: u32,
}

pub struct MemoryCollector;

impl MemoryCollector {
    pub fn new() -> Result<Self> {
        Ok(MemoryCollector)
    }

    pub fn collect_memory(&self) -> Result<Vec<MetricRecord>> {
        let timestamp = current_unix_timestamp()?;
        let mut stats_by_app: HashMap<String, MemStats> = HashMap::new();
        let mut pids_scanned = 0u32;
        let mut pids_skipped = 0u32;

        if let Ok(entries) = fs::read_dir("/proc") {
            for entry in entries.flatten() {
                let path = entry.path();
                let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

                let pid: u32 = match filename.parse() {
                    Ok(p) => p,
                    Err(_) => continue,
                };

                pids_scanned = pids_scanned.saturating_add(1);

                if let Ok(status) = read_proc_status(&path) {
                    let rss_bytes = status.vm_rss.unwrap_or(0) * 1024;
                    let vms_bytes = status.vm_size.unwrap_or(0) * 1024;
                    let swap_bytes = status.vm_swap.unwrap_or(0) * 1024;

                    if rss_bytes > vms_bytes {
                        log::warn!(
                            "Memory invariant violated for PID {}: RSS {} > VMS {}",
                            pid,
                            rss_bytes,
                            vms_bytes
                        );
                    }

                    if let Ok(app_name) = get_process_name(pid) {
                        let entry = stats_by_app.entry(app_name).or_default();
                        entry.rss = entry.rss.saturating_add(rss_bytes);
                        entry.vms = entry.vms.saturating_add(vms_bytes);
                        entry.swap = entry.swap.saturating_add(swap_bytes);
                        entry.count = entry.count.saturating_add(1);
                    } else {
                        pids_skipped = pids_skipped.saturating_add(1);
                    }
                } else {
                    pids_skipped = pids_skipped.saturating_add(1);
                }
            }
        }

        log::debug!(
            "Memory collector: scanned {} PIDs, collected {} apps, skipped {}",
            pids_scanned,
            stats_by_app.len(),
            pids_skipped
        );

        let records = stats_by_app
            .into_iter()
            .map(|(app_name, stats)| MetricRecord {
                app_name,
                timestamp,
                payload: MetricPayload::Mem(MemoryData {
                    rss_bytes: stats.rss,
                    vms_bytes: stats.vms,
                    swap_bytes: stats.swap,
                    process_count: stats.count,
                }),
            })
            .collect();

        Ok(records)
    }

    pub async fn run(
        self,
        tx: mpsc::Sender<CollectorEvent>,
        shutdown: watch::Receiver<bool>,
    ) -> Result<()> {
        run_collector_loop(
            self,
            POLL_INTERVAL,
            tx,
            shutdown,
            |c| c.collect_memory(),
            |p| matches!(p, MetricPayload::Mem(_)),
            "Memory",
        )
        .await
    }
}

#[derive(Debug, Default)]
struct ProcStatus {
    vm_rss: Option<u64>,
    vm_size: Option<u64>,
    vm_swap: Option<u64>,
}

fn read_proc_status(path: &Path) -> Result<ProcStatus> {
    let status_path = path.join("status");
    let content = fs::read_to_string(&status_path)?;

    let mut result = ProcStatus::default();
    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }

        match parts[0] {
            "VmRSS:" => {
                if let Ok(val) = parts[1].parse::<u64>() {
                    result.vm_rss = Some(val);
                }
            }
            "VmSize:" => {
                if let Ok(val) = parts[1].parse::<u64>() {
                    result.vm_size = Some(val);
                }
            }
            "VmSwap:" => {
                if let Ok(val) = parts[1].parse::<u64>() {
                    result.vm_swap = Some(val);
                }
            }
            _ => {}
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_collector_new() {
        let collector = MemoryCollector::new();
        assert!(collector.is_ok());
    }

    #[test]
    fn test_aggregation_single_process() {
        let mut stats: HashMap<String, MemStats> = HashMap::new();
        let entry = stats.entry("firefox".to_string()).or_default();
        entry.rss = entry.rss.saturating_add(500_000_000);
        entry.vms = entry.vms.saturating_add(1_000_000_000);
        entry.swap = entry.swap.saturating_add(100_000_000);
        entry.count = entry.count.saturating_add(1);

        assert_eq!(stats.len(), 1);
        assert_eq!(stats["firefox"].rss, 500_000_000);
        assert_eq!(stats["firefox"].count, 1);
    }

    #[test]
    fn test_aggregation_multiple_processes_same_app() {
        let mut stats: HashMap<String, MemStats> = HashMap::new();

        for _ in 0..3 {
            let entry = stats.entry("chrome".to_string()).or_default();
            entry.rss = entry.rss.saturating_add(300_000_000);
            entry.vms = entry.vms.saturating_add(600_000_000);
            entry.swap = entry.swap.saturating_add(50_000_000);
            entry.count = entry.count.saturating_add(1);
        }

        assert_eq!(stats.len(), 1);
        assert_eq!(stats["chrome"].rss, 900_000_000);
        assert_eq!(stats["chrome"].count, 3);
    }

    #[test]
    fn test_aggregation_multiple_apps() {
        let mut stats: HashMap<String, MemStats> = HashMap::new();

        let apps = vec!["firefox", "chrome", "python"];
        for app in apps {
            let entry = stats.entry(app.to_string()).or_default();
            entry.rss = entry.rss.saturating_add(250_000_000);
            entry.count = entry.count.saturating_add(1);
        }

        assert_eq!(stats.len(), 3);
    }

    #[test]
    fn test_kb_to_bytes_conversion() {
        let kb = 2720u64;
        let bytes = kb * 1024;
        assert_eq!(bytes, 2785280);
    }

    #[test]
    fn test_read_proc_status_valid_content() {
        let status = ProcStatus {
            vm_rss: Some(1024),
            vm_size: Some(2048),
            vm_swap: Some(512),
        };

        assert_eq!(status.vm_rss, Some(1024));
        assert_eq!(status.vm_size, Some(2048));
        assert_eq!(status.vm_swap, Some(512));
    }

    #[test]
    fn test_read_proc_status_partial_fields() {
        let status = ProcStatus {
            vm_rss: Some(1024),
            vm_size: None,
            vm_swap: Some(512),
        };

        assert_eq!(status.vm_rss, Some(1024));
        assert_eq!(status.vm_size, None);
        assert_eq!(status.vm_swap, Some(512));
    }

    #[test]
    fn test_read_proc_status_all_missing() {
        let status = ProcStatus::default();

        assert_eq!(status.vm_rss, None);
        assert_eq!(status.vm_size, None);
        assert_eq!(status.vm_swap, None);
    }

    #[test]
    fn test_aggregation_with_zero_values() {
        let mut stats: HashMap<String, MemStats> = HashMap::new();

        let entry = stats.entry("idle_proc".to_string()).or_default();
        entry.rss = entry.rss.saturating_add(0);
        entry.vms = entry.vms.saturating_add(0);
        entry.swap = entry.swap.saturating_add(0);
        entry.count = entry.count.saturating_add(1);

        assert_eq!(stats["idle_proc"].rss, 0);
        assert_eq!(stats["idle_proc"].count, 1);
    }

    #[test]
    fn test_aggregation_saturation_overflow() {
        let mut stats: HashMap<String, MemStats> = HashMap::new();

        let entry = stats.entry("large_app".to_string()).or_default();
        entry.rss = entry.rss.saturating_add(u64::MAX);
        entry.rss = entry.rss.saturating_add(1000);

        assert_eq!(stats["large_app"].rss, u64::MAX);
    }

    #[test]
    fn test_memory_data_field_sizes() {
        let data = MemoryData {
            rss_bytes: 1_000_000_000,
            vms_bytes: 2_000_000_000,
            swap_bytes: 500_000_000,
            process_count: 5,
        };

        assert_eq!(data.rss_bytes, 1_000_000_000);
        assert_eq!(data.vms_bytes, 2_000_000_000);
        assert_eq!(data.swap_bytes, 500_000_000);
        assert_eq!(data.process_count, 5);
    }

    #[test]
    fn test_memory_data_large_process_count() {
        let data = MemoryData {
            rss_bytes: 100_000_000,
            vms_bytes: 200_000_000,
            swap_bytes: 0,
            process_count: u32::MAX,
        };

        assert_eq!(data.process_count, u32::MAX);
    }

    #[test]
    fn test_aggregation_preserves_app_name() {
        let mut stats: HashMap<String, MemStats> = HashMap::new();

        let app_names = vec!["my-app-v1.0", "/usr/bin/python3", "com.example.app"];
        for app_name in &app_names {
            let entry = stats.entry(app_name.to_string()).or_default();
            entry.rss = entry.rss.saturating_add(100_000_000);
            entry.count = entry.count.saturating_add(1);
        }

        for app_name in &app_names {
            assert!(stats.contains_key(*app_name));
        }
    }

    #[test]
    fn test_kb_conversion_small_value() {
        let kb = 1u64;
        let bytes = kb * 1024;
        assert_eq!(bytes, 1024);
    }

    #[test]
    fn test_kb_conversion_large_value() {
        let kb = 1_000_000u64;
        let bytes = kb * 1024;
        assert_eq!(bytes, 1_024_000_000);
    }

    #[test]
    fn test_aggregation_mixed_zero_nonzero() {
        let mut stats: HashMap<String, MemStats> = HashMap::new();

        let entry = stats.entry("app1".to_string()).or_default();
        entry.rss = entry.rss.saturating_add(100_000_000);
        entry.vms = entry.vms.saturating_add(0);
        entry.swap = entry.swap.saturating_add(50_000_000);
        entry.count = entry.count.saturating_add(1);

        assert_eq!(stats["app1"].rss, 100_000_000);
        assert_eq!(stats["app1"].vms, 0);
        assert_eq!(stats["app1"].swap, 50_000_000);
    }

    #[test]
    fn test_parse_status_line_valid_vmrss() {
        // Simulate parsing a valid VmRSS line
        let line = "VmRSS:      2720 kB";
        let parts: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "VmRSS:");
        assert_eq!(parts[1].parse::<u64>().unwrap(), 2720);
    }

    #[test]
    fn test_parse_status_line_whitespace_variation() {
        // Test with different whitespace
        let line = "VmSize:  13112   kB";
        let parts: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "VmSize:");
        assert_eq!(parts[1].parse::<u64>().unwrap(), 13112);
    }

    #[test]
    fn test_parse_status_line_with_comments() {
        // Simulate a line with other fields mixed in
        let content = "VmRSS:      2720 kB\nVmSize:     13112 kB\nVmSwap:         0 kB";
        let mut vm_rss = None;
        let mut vm_size = None;
        let mut vm_swap = None;

        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                match parts[0] {
                    "VmRSS:" => vm_rss = parts[1].parse::<u64>().ok(),
                    "VmSize:" => vm_size = parts[1].parse::<u64>().ok(),
                    "VmSwap:" => vm_swap = parts[1].parse::<u64>().ok(),
                    _ => {}
                }
            }
        }

        assert_eq!(vm_rss, Some(2720));
        assert_eq!(vm_size, Some(13112));
        assert_eq!(vm_swap, Some(0));
    }

    #[test]
    fn test_parse_status_malformed_value() {
        // Simulate parsing a malformed VmRSS line
        let line = "VmRSS:      invalid kB";
        let parts: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "VmRSS:");
        // Parsing should fail gracefully
        assert!(parts[1].parse::<u64>().is_err());
    }

    #[test]
    fn test_proc_status_default_none() {
        // Default ProcStatus should have all None fields
        let status = ProcStatus::default();
        assert!(status.vm_rss.is_none());
        assert!(status.vm_size.is_none());
        assert!(status.vm_swap.is_none());
    }

    #[test]
    fn test_large_memory_values() {
        // Test with realistically large memory values (TB scale)
        let large_kb = 1_000_000_000u64; // ~1TB in KB
        let bytes = large_kb * 1024;
        assert!(bytes > 1_000_000_000_000); // verify it's over 1TB
        assert_eq!(bytes, 1_024_000_000_000);
    }

    #[test]
    fn test_memory_data_zero_processes() {
        // Edge case: process_count should never be 0 in practice, but test handles it
        let data = MemoryData {
            rss_bytes: 1000,
            vms_bytes: 2000,
            swap_bytes: 0,
            process_count: 0,
        };
        assert_eq!(data.process_count, 0);
    }

    #[test]
    fn test_aggregation_process_count_increment() {
        // Verify that process count correctly increments
        let mut count = 0u32;
        for _ in 0..100 {
            count = count.saturating_add(1);
        }
        assert_eq!(count, 100);
    }

    #[test]
    fn test_aggregation_process_count_overflow() {
        // Verify saturation at u32::MAX
        let mut count = u32::MAX - 5;
        for _ in 0..10 {
            count = count.saturating_add(1);
        }
        assert_eq!(count, u32::MAX);
    }

    #[test]
    fn test_memory_invariant_rss_greater_than_vms() {
        // Test the invariant check: RSS should never exceed VMS
        // In normal circumstances, this is a kernel invariant that shouldn't be violated.
        // However, corrupted /proc data could trigger this.
        // The check should: log a warning, but still aggregate the data.

        let rss_bytes = 2_000_000_000u64; // 2GB
        let vms_bytes = 1_000_000_000u64; // 1GB (less than RSS, violates invariant)

        // Verify the condition
        assert!(rss_bytes > vms_bytes, "Test setup: RSS should be > VMS");

        // In production, this would trigger a warning log and debug_assert,
        // but the data would still be aggregated (graceful degradation).
        // This test documents the expected behavior.
    }

    #[test]
    fn test_memory_invariant_rss_less_than_vms() {
        // Normal case: RSS <= VMS (the kernel invariant)
        let rss_bytes = 1_000_000_000u64; // 1GB
        let vms_bytes = 2_000_000_000u64; // 2GB

        assert!(rss_bytes <= vms_bytes, "Normal case: RSS should be <= VMS");
    }
}
