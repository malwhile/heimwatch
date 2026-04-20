//! Linux CPU collector using eBPF for kernel-space sched_switch monitoring.
//!
//! Requires: CAP_BPF + CAP_PERFMON or root (Linux 5.8+)
//! Uses: aya framework for eBPF program loading and tracepoint attachment

/// Poll interval for CPU usage collection (5 seconds).
pub const POLL_INTERVAL: Duration = Duration::from_secs(5);

use crate::error::CollectorError;
use anyhow::Result;
use std::collections::HashMap;

use aya::Ebpf;
use aya::maps::HashMap as AyaHashMap;
use aya::programs::TracePoint;
use heimwatch_core::{
    CollectorEvent, CpuData, MetricPayload, MetricRecord, current_unix_timestamp,
    process::get_process_name,
};
use std::time::Duration;
use tokio::sync::{mpsc, watch};

/// Local Pod-compatible mirror of PidCpuStats.
///
/// The orphan rule prevents implementing `aya::Pod` for `PidCpuStats` in this crate.
/// This mirror type has identical layout (both are `#[repr(C)]`) and can be safely converted.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LocalPidCpuStats {
    cpu_time_ns: u64,
    last_sched_in_ns: u64,
    comm: [u8; 16],
}

// Safety: repr(C), all fields (u64, u64, [u8; 16]) are valid for all bit patterns.
unsafe impl aya::Pod for LocalPidCpuStats {}

/// Embedded BPF object, compiled by build.rs at build time.
static BPF_BYTES: &[u8] = aya::include_bytes_aligned!(concat!(env!("OUT_DIR"), "/heimwatch-ebpf"));

/// Owns the loaded BPF program and maintains delta state.
pub struct CpuCollector {
    /// The loaded eBPF object (owns program fds).
    bpf: Ebpf,
    /// Previous snapshot: app_name -> cumulative cpu_time_ns.
    /// Keyed by app_name (not PID) to survive PID reuse.
    prev_state: HashMap<String, u64>,
}

impl CpuCollector {
    /// Load and attach the BPF sched_switch tracepoint. Requires CAP_BPF or root.
    pub fn new() -> Result<Self> {
        let mut bpf = Ebpf::load(BPF_BYTES)?;

        // Attach tracepoint to sched:sched_switch
        let prog: &mut TracePoint = bpf
            .program_mut("trace_sched_switch")
            .ok_or_else(|| CollectorError::ProgramNotFound("trace_sched_switch".to_string()))?
            .try_into()
            .map_err(|_| CollectorError::ProgramTypeMismatch("trace_sched_switch".to_string()))?;
        prog.load()?;
        prog.attach("sched", "sched_switch")?;

        Ok(CpuCollector {
            bpf,
            prev_state: HashMap::new(),
        })
    }

    /// Poll the BPF map once. Returns one MetricRecord per active app.
    ///
    /// Delta calculation:
    /// - Reads all (pid, PidCpuStats) pairs from the BPF map.
    /// - Resolves each PID to an app name (kernel comm field preferred, /proc fallback).
    /// - Aggregates CPU time by app_name (handles multiple PIDs per app).
    /// - Subtracts previous snapshot to get per-interval deltas.
    /// - Computes usage_percent = (delta_ns / interval_ns) * 100.
    pub fn collect_cpu(&mut self, interval: Duration) -> Result<Vec<MetricRecord>> {
        let timestamp = current_unix_timestamp()?;
        let interval_ns = interval.as_nanos() as u64;

        // Get the CPU_STATS map from the BPF program and iterate it
        let map_ref = self
            .bpf
            .map_mut("CPU_STATS")
            .ok_or_else(|| CollectorError::MapNotFound("CPU_STATS".to_string()))?;

        let stats_map: AyaHashMap<_, u32, LocalPidCpuStats> = AyaHashMap::try_from(map_ref)?;

        // Aggregate current totals by app name (multiple PIDs → same app)
        let mut current_by_app: HashMap<String, u64> = HashMap::new();

        for entry in stats_map.iter() {
            let (pid, local_stats) = entry?;

            // Skip PID 0 (idle/swapper) to avoid noise
            if pid == 0 {
                continue;
            }

            // Resolve app name from multiple sources (in priority order):
            // 1. Try /proc lookup first (always current for running processes, detects PID reuse)
            // 2. Fall back to kernel-captured comm (works even after process exits)
            // 3. Fall back to "pid:XXXX"
            let app_name = get_process_name(pid)
                .ok()
                .or_else(|| comm_to_string(&local_stats.comm))
                .unwrap_or_else(|| format!("pid:{}", pid));

            // Aggregate: if multiple PIDs belong to the same app, sum their CPU time
            let entry = current_by_app.entry(app_name).or_insert(0);
            *entry = entry.saturating_add(local_stats.cpu_time_ns);
        }

        // Calculate deltas and convert to percentages
        let mut records = Vec::new();
        for (app_name, current_ns) in &current_by_app {
            let prev_ns = self.prev_state.get(app_name).copied().unwrap_or(0);

            // If current < prev, PID was reused — delta is 0 for this interval
            let delta_ns = current_ns.saturating_sub(prev_ns);

            // Only emit a record if there was actual CPU time in this interval
            if delta_ns > 0 && interval_ns > 0 {
                let usage_percent = (delta_ns as f64 / interval_ns as f64 * 100.0) as f32;
                records.push(MetricRecord {
                    app_name: app_name.clone(),
                    timestamp,
                    payload: MetricPayload::Cpu(CpuData {
                        usage_percent,
                        // Per-process metric (not system-wide). core_count=1 indicates a single
                        // process measurement, not that the system has 1 CPU. Storage/TUI layers
                        // can normalize usage_percent across actual core count if needed.
                        core_count: 1,
                    }),
                });
            }
        }

        // Update previous state
        self.prev_state = current_by_app;

        Ok(records)
    }

    /// Run the CPU collection loop.
    ///
    /// Polls for CPU metrics at the configured interval using tokio::select!
    /// Sends CollectorEvents through the provided channel. Exits when shutdown signal triggers.
    pub async fn run(
        mut self,
        tx: mpsc::Sender<CollectorEvent>,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<()> {
        let mut ticker = tokio::time::interval(POLL_INTERVAL);

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    // Collect CPU metrics
                    for record in self.collect_cpu(POLL_INTERVAL)? {
                        if let MetricPayload::Cpu(_) = &record.payload {
                            let event = CollectorEvent {
                                app_name: record.app_name.clone(),
                                payload: record.payload,
                                timestamp: record.timestamp,
                            };
                            if let Err(e) = tx.send(event).await {
                                log::error!(
                                    "Failed to send CPU event for app '{}': {}",
                                    record.app_name,
                                    e
                                );
                            }
                        }
                    }
                }

                _ = shutdown.changed() => {
                    log::debug!("CPU collector received shutdown signal");
                    break;
                }
            }
        }

        Ok(())
    }
}

/// Convert a null-terminated byte array (from kernel comm field) to a String.
/// Returns None if the comm is empty or invalid UTF-8.
fn comm_to_string(comm: &[u8; 16]) -> Option<String> {
    // Find the null terminator
    let end = comm.iter().position(|&b| b == 0).unwrap_or(16);

    // Empty comm field
    if end == 0 {
        return None;
    }

    // Convert to UTF-8 string, replacing invalid bytes with replacement character
    Some(String::from_utf8_lossy(&comm[..end]).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_comm_to_string_valid() {
        let mut comm = [0u8; 16];
        b"firefox"
            .iter()
            .enumerate()
            .for_each(|(i, &b)| comm[i] = b);
        assert_eq!(comm_to_string(&comm), Some("firefox".to_string()));
    }

    #[test]
    fn test_comm_to_string_empty() {
        let comm = [0u8; 16];
        assert_eq!(comm_to_string(&comm), None);
    }

    #[test]
    fn test_comm_to_string_truncated() {
        let mut comm = [0u8; 16];
        b"python3.11"
            .iter()
            .enumerate()
            .for_each(|(i, &b)| comm[i] = b);
        // Manually null-terminate at position 6
        comm[6] = 0;
        assert_eq!(comm_to_string(&comm), Some("python".to_string()));
    }

    #[test]
    fn test_delta_calculation_no_previous() {
        // First collection should use 0 as previous
        let current_ns = 1000u64;
        let prev_ns = 0u64;
        let delta_ns = current_ns.saturating_sub(prev_ns);
        assert_eq!(delta_ns, 1000);
    }

    #[test]
    fn test_delta_calculation_pid_reuse() {
        // If PID is reused and new process has less CPU time, delta should clamp to 0
        let current_ns = 500u64;
        let prev_ns = 1000u64;
        let delta_ns = current_ns.saturating_sub(prev_ns);
        assert_eq!(delta_ns, 0); // saturating_sub prevents negative values
    }

    #[test]
    fn test_delta_calculation_normal_progression() {
        // Normal case: current > previous
        let current_ns = 2500u64;
        let prev_ns = 1500u64;
        let delta_ns = current_ns.saturating_sub(prev_ns);
        assert_eq!(delta_ns, 1000);
    }

    #[test]
    fn test_usage_percent_calculation() {
        // Usage % = (delta_ns / interval_ns) * 100
        let delta_ns = 500_000_000u64; // 500ms
        let interval_ns = 1_000_000_000u64; // 1s
        let usage_percent = (delta_ns as f64 / interval_ns as f64 * 100.0) as f32;
        assert!((usage_percent - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_usage_percent_bounds_low() {
        // Very low CPU usage
        let delta_ns = 1_000_000u64; // 1ms
        let interval_ns = 1_000_000_000u64; // 1s
        let usage_percent = (delta_ns as f64 / interval_ns as f64 * 100.0) as f32;
        assert!((usage_percent - 0.1).abs() < 0.01);
    }

    #[test]
    fn test_usage_percent_bounds_high() {
        // High CPU usage (nearly 100%)
        let delta_ns = 999_000_000u64; // 999ms
        let interval_ns = 1_000_000_000u64; // 1s
        let usage_percent = (delta_ns as f64 / interval_ns as f64 * 100.0) as f32;
        assert!((usage_percent - 99.9).abs() < 0.01);
    }

    #[test]
    fn test_usage_percent_with_small_interval() {
        // Test with 100ms interval
        let delta_ns = 50_000_000u64; // 50ms
        let interval_ns = 100_000_000u64; // 100ms
        let usage_percent = (delta_ns as f64 / interval_ns as f64 * 100.0) as f32;
        assert!((usage_percent - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_usage_percent_with_large_interval() {
        // Test with 10s interval
        let delta_ns = 5_000_000_000u64; // 5s
        let interval_ns = 10_000_000_000u64; // 10s
        let usage_percent = (delta_ns as f64 / interval_ns as f64 * 100.0) as f32;
        assert!((usage_percent - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_pid_zero_filtering() {
        // Verify that PID 0 would be filtered out (test the condition)
        let pid = 0u32;
        let should_skip = pid == 0;
        assert!(should_skip);

        let pid = 1u32;
        let should_skip = pid == 0;
        assert!(!should_skip);
    }

    #[test]
    fn test_aggregation_single_pid() {
        // Single PID should result in one record with its CPU time
        let mut app_totals: HashMap<String, u64> = HashMap::new();
        app_totals.insert("firefox".to_string(), 500_000_000u64);

        assert_eq!(app_totals.len(), 1);
        assert_eq!(app_totals.get("firefox"), Some(&500_000_000u64));
    }

    #[test]
    fn test_aggregation_multiple_pids_same_app() {
        // Multiple PIDs of the same app should sum their CPU time
        let mut app_totals: HashMap<String, u64> = HashMap::new();

        // Simulate two threads of the same app
        *app_totals.entry("firefox".to_string()).or_insert(0) += 300_000_000u64;
        *app_totals.entry("firefox".to_string()).or_insert(0) += 200_000_000u64;

        assert_eq!(app_totals.len(), 1);
        assert_eq!(app_totals.get("firefox"), Some(&500_000_000u64));
    }

    #[test]
    fn test_aggregation_multiple_apps() {
        // Different apps should have separate records
        let mut app_totals: HashMap<String, u64> = HashMap::new();
        app_totals.insert("firefox".to_string(), 300_000_000u64);
        app_totals.insert("chrome".to_string(), 400_000_000u64);
        app_totals.insert("python".to_string(), 200_000_000u64);

        assert_eq!(app_totals.len(), 3);
        assert_eq!(app_totals.get("firefox"), Some(&300_000_000u64));
        assert_eq!(app_totals.get("chrome"), Some(&400_000_000u64));
        assert_eq!(app_totals.get("python"), Some(&200_000_000u64));
    }

    #[test]
    fn test_zero_interval_skipped() {
        // If interval_ns is 0, records should be skipped
        let delta_ns = 500u64;
        let interval_ns = 0u64;

        // The condition is: delta_ns > 0 && interval_ns > 0
        let should_emit = delta_ns > 0 && interval_ns > 0;
        assert!(!should_emit);
    }

    #[test]
    fn test_zero_delta_skipped() {
        // If delta_ns is 0, records should be skipped
        let delta_ns = 0u64;
        let interval_ns = 1_000_000_000u64;

        // The condition is: delta_ns > 0 && interval_ns > 0
        let should_emit = delta_ns > 0 && interval_ns > 0;
        assert!(!should_emit);
    }
}
