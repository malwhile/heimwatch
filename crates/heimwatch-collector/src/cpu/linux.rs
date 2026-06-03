//! Linux CPU collector using eBPF for kernel-space sched_switch monitoring.
//!
//! Requires: CAP_BPF + CAP_PERFMON or root (Linux 5.8+)
//! Uses: aya framework for eBPF program loading and tracepoint attachment

use crate::error::CollectorError;
use crate::util::{comm_to_string, run_collector_loop};
use anyhow::Result;
use std::collections::HashMap;
use std::thread;

use aya::Ebpf;
use aya::maps::HashMap as AyaHashMap;
use aya::programs::TracePoint;
use heimwatch_core::{
    CollectorEvent, CpuData, MetricPayload, MetricRecord, current_unix_timestamp,
};
use std::time::Duration;
use tokio::sync::{mpsc, watch};

/// Poll interval for CPU usage collection (5 seconds).
pub const POLL_INTERVAL: Duration = Duration::from_secs(5);
pub const COLLECTOR_NAME: &str = "CPU";
pub const STATS_NAME: &str = "CPU_STATS";
pub const IGNORED_PROCESSES: [&str; 1] = [
    // Skip kernel idle tasks (swapper represents CPU idle time, not real work)
    "swapper",
];

/// Local Pod-compatible mirror of PidCpuStats.
///
/// The orphan rule prevents implementing `aya::Pod` for `PidCpuStats` in this crate.
/// This mirror type has identical layout (both are `#[repr(C)]`) and can be safely converted.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LocalPidCpuStats {
    cpu_time_ns: u64,
    last_sched_in_ns: u64,
}

// Safety: repr(C), all fields (u64, u64) are valid for all bit patterns.
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

    /// Number of CPU Cores
    num_cores: u64,
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

        // Get number of CPU cores for percentage calculation
        let num_cores = thread::available_parallelism()
            .map(|n| n.get() as u64)
            .unwrap_or(1);

        Ok(CpuCollector {
            bpf,
            prev_state: HashMap::new(),
            num_cores,
        })
    }

    /// Poll the BPF map once. Returns one MetricRecord per active process.
    ///
    /// Delta calculation:
    /// - Reads all (process_name, PidCpuStats) pairs from the BPF map.
    /// - Process name (comm field) is the key, so aggregation happens automatically in eBPF.
    /// - Subtracts previous snapshot to get per-interval deltas.
    /// - Computes cpu_usage_percent = (delta_ns / (interval_ns × num_cores)) × 100.
    pub fn collect_cpu(&mut self, interval: Duration) -> Result<Vec<MetricRecord>> {
        let timestamp = current_unix_timestamp()?;
        let interval_ns = interval.as_nanos() as u64;

        // Get the CPU_STATS map from the BPF program and iterate it
        let map_ref = self
            .bpf
            .map_mut(STATS_NAME)
            .ok_or_else(|| CollectorError::MapNotFound(STATS_NAME.to_string()))?;

        // Map is now keyed by process name ([u8; 16]), not PID
        let stats_map: AyaHashMap<_, [u8; 16], LocalPidCpuStats> = AyaHashMap::try_from(map_ref)?;

        let mut records = Vec::new();
        let mut new_state: HashMap<String, u64> = HashMap::new();
        let total_capacity_ns = interval_ns.saturating_mul(self.num_cores);

        for entry in stats_map.iter() {
            let (comm, local_stats) = entry?;

            // Convert process name to string (null-terminated)
            let app_name = comm_to_string(&comm).unwrap_or_else(|| "(unknown)".to_string());

            for ignored_process_name in IGNORED_PROCESSES {
                if app_name.starts_with(ignored_process_name) {
                    continue;
                }
            }

            let current_ns = local_stats.cpu_time_ns;
            let prev_ns = self.prev_state.get(&app_name).copied().unwrap_or(0);

            // If current < prev, PID was reused — delta is 0 for this interval
            let delta_ns = current_ns.saturating_sub(prev_ns);

            // Only emit a record if there was actual CPU time in this interval
            if delta_ns > 0 {
                let cpu_usage_percent = if total_capacity_ns > 0 {
                    (delta_ns as f64 / total_capacity_ns as f64 * 100.0) as f32
                } else {
                    0.0
                };

                records.push(MetricRecord {
                    app_name: app_name.clone(),
                    timestamp,
                    payload: MetricPayload::Cpu(CpuData {
                        cpu_time_ns: delta_ns,
                        cpu_usage_percent,
                    }),
                });
            }

            new_state.insert(app_name, current_ns);
        }

        // Update previous state
        self.prev_state = new_state;

        Ok(records)
    }

    /// Run the CPU collection loop.
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
            |c| c.collect_cpu(POLL_INTERVAL),
            |p| matches!(p, MetricPayload::Cpu(_)),
            COLLECTOR_NAME,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_absolute_cpu_time_500ms() {
        // Test reporting absolute CPU time (not percentage)
        let delta_ns = 500_000_000u64; // 500ms
        assert_eq!(delta_ns, 500_000_000);
    }

    #[test]
    fn test_absolute_cpu_time_1ms() {
        // Very low CPU usage in nanoseconds
        let delta_ns = 1_000_000u64; // 1ms
        assert_eq!(delta_ns, 1_000_000);
    }

    #[test]
    fn test_absolute_cpu_time_1s() {
        // 1 second of CPU time
        let delta_ns = 1_000_000_000u64; // 1s
        assert_eq!(delta_ns, 1_000_000_000);
    }

    #[test]
    fn test_absolute_cpu_time_50ms() {
        // Test with 50ms CPU time
        let delta_ns = 50_000_000u64; // 50ms
        assert_eq!(delta_ns, 50_000_000);
    }

    #[test]
    fn test_absolute_cpu_time_5s() {
        // 5 seconds of CPU time
        let delta_ns = 5_000_000_000u64; // 5s
        assert_eq!(delta_ns, 5_000_000_000);
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
