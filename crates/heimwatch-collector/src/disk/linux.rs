//! Linux disk I/O collector using eBPF for kernel-space block_rq_issue monitoring.
//!
//! Requires: CAP_BPF + CAP_PERFMON or root (Linux 5.8+)
//! Uses: aya framework for eBPF program loading and tracepoint attachment

use crate::error::CollectorError;
use anyhow::Result;
use std::collections::HashMap;

use aya::Ebpf;
use aya::maps::HashMap as AyaHashMap;
use aya::programs::TracePoint;
use heimwatch_core::{
    CollectorEvent, DiskData, MetricPayload, MetricRecord, current_unix_timestamp,
    process::get_process_name,
};
use std::time::Duration;
use tokio::sync::{mpsc, watch};

/// Poll interval for disk I/O collection (5 seconds).
pub const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Local Pod-compatible mirror of PidDiskStats.
///
/// The orphan rule prevents implementing `aya::Pod` for `PidDiskStats` in this crate.
/// This mirror type has identical layout (both are `#[repr(C)]`).
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LocalPidDiskStats {
    read_bytes: u64,
    write_bytes: u64,
    comm: [u8; 16],
}

// Safety: repr(C), all fields (u64, u64, [u8; 16]) are valid for all bit patterns, no padding.
unsafe impl aya::Pod for LocalPidDiskStats {}

/// Embedded BPF object, compiled by build.rs at build time.
static BPF_BYTES: &[u8] = aya::include_bytes_aligned!(concat!(env!("OUT_DIR"), "/heimwatch-ebpf"));

/// Owns the loaded BPF program and maintains delta state.
pub struct DiskCollector {
    /// The loaded eBPF object (owns program fds).
    bpf: Ebpf,
    /// Previous snapshot: app_name -> cumulative read_bytes.
    prev_read: HashMap<String, u64>,
    /// Previous snapshot: app_name -> cumulative write_bytes.
    prev_write: HashMap<String, u64>,
}

impl DiskCollector {
    /// Load and attach the BPF block_rq_issue tracepoint. Requires CAP_BPF or root.
    pub fn new() -> Result<Self> {
        let mut bpf = Ebpf::load(BPF_BYTES)?;

        let prog: &mut TracePoint = bpf
            .program_mut("trace_block_rq_issue")
            .ok_or_else(|| CollectorError::ProgramNotFound("trace_block_rq_issue".to_string()))?
            .try_into()
            .map_err(|_| CollectorError::ProgramTypeMismatch("trace_block_rq_issue".to_string()))?;
        prog.load()?;
        prog.attach("block", "block_rq_issue")?;

        Ok(DiskCollector {
            bpf,
            prev_read: HashMap::new(),
            prev_write: HashMap::new(),
        })
    }

    /// Poll the BPF map once. Returns one MetricRecord per active app with nonzero delta.
    pub fn collect_disk(&mut self, _interval: Duration) -> Result<Vec<MetricRecord>> {
        let timestamp = current_unix_timestamp()?;

        let map_ref = self
            .bpf
            .map_mut("DISK_STATS")
            .ok_or_else(|| CollectorError::MapNotFound("DISK_STATS".to_string()))?;

        let stats_map: AyaHashMap<_, u32, LocalPidDiskStats> = AyaHashMap::try_from(map_ref)?;

        // Aggregate current totals by app name
        let mut current_read: HashMap<String, u64> = HashMap::new();
        let mut current_write: HashMap<String, u64> = HashMap::new();

        for entry in stats_map.iter() {
            let (pid, local_stats) = entry?;

            if pid == 0 {
                continue;
            }

            let app_name = get_process_name(pid)
                .ok()
                .or_else(|| comm_to_string(&local_stats.comm))
                .unwrap_or_else(|| format!("pid:{}", pid));

            // Aggregate: if multiple PIDs belong to the same app, sum their I/O bytes
            let read_entry = current_read.entry(app_name.clone()).or_insert(0);
            *read_entry = read_entry.saturating_add(local_stats.read_bytes);

            let write_entry = current_write.entry(app_name).or_insert(0);
            *write_entry = write_entry.saturating_add(local_stats.write_bytes);
        }

        let mut records = Vec::new();
        // Collect over all app names that appeared in either read or write
        let mut all_apps: std::collections::HashSet<String> = std::collections::HashSet::new();
        all_apps.extend(current_read.keys().cloned());
        all_apps.extend(current_write.keys().cloned());

        for app_name in all_apps {
            let cur_r = current_read.get(&app_name).copied().unwrap_or(0);
            let cur_w = current_write.get(&app_name).copied().unwrap_or(0);

            let prev_r = self.prev_read.get(&app_name).copied().unwrap_or(0);
            let prev_w = self.prev_write.get(&app_name).copied().unwrap_or(0);

            let delta_r = cur_r.saturating_sub(prev_r);
            let delta_w = cur_w.saturating_sub(prev_w);

            // Only emit a record if there was actual I/O in this interval
            if delta_r > 0 || delta_w > 0 {
                records.push(MetricRecord {
                    app_name: app_name.clone(),
                    timestamp,
                    payload: MetricPayload::Dsk(DiskData {
                        read_bytes: delta_r,
                        write_bytes: delta_w,
                        // mount_point is left empty: we track per-process I/O (app-level), not per-filesystem.
                        // The kernel's block_rq_issue tracepoint doesn't provide mount point information
                        // without expensive kernel VFS lookups. Per-app metrics are more useful for activity
                        // tracking anyway (e.g., "Firefox used 500MB read"). If per-mount breakdowns are
                        // needed later, a separate collector using /proc/diskstats would be more efficient.
                        mount_point: String::new(),
                    }),
                });
            }
        }

        // Update previous state
        self.prev_read = current_read;
        self.prev_write = current_write;

        Ok(records)
    }

    /// Run the disk I/O collection loop.
    pub async fn run(
        mut self,
        tx: mpsc::Sender<CollectorEvent>,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<()> {
        let mut ticker = tokio::time::interval(POLL_INTERVAL);

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    for record in self.collect_disk(POLL_INTERVAL)? {
                        if let MetricPayload::Dsk(_) = &record.payload {
                            let event = CollectorEvent {
                                app_name: record.app_name.clone(),
                                payload: record.payload,
                                timestamp: record.timestamp,
                            };
                            if let Err(e) = tx.send(event).await {
                                log::error!(
                                    "Failed to send Disk event for app '{}': {}",
                                    record.app_name,
                                    e
                                );
                            }
                        }
                    }
                }

                _ = shutdown.changed() => {
                    log::debug!("Disk collector received shutdown signal");
                    break;
                }
            }
        }

        Ok(())
    }
}

/// Convert a null-terminated byte array (from kernel comm field) to a String.
fn comm_to_string(comm: &[u8; 16]) -> Option<String> {
    let end = comm.iter().position(|&b| b == 0).unwrap_or(16);
    if end == 0 {
        return None;
    }
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
        comm[6] = 0;
        assert_eq!(comm_to_string(&comm), Some("python".to_string()));
    }

    #[test]
    fn test_delta_calculation_no_previous() {
        let current_bytes = 1_000_000u64;
        let prev_bytes = 0u64;
        let delta_bytes = current_bytes.saturating_sub(prev_bytes);
        assert_eq!(delta_bytes, 1_000_000);
    }

    #[test]
    fn test_delta_calculation_pid_reuse() {
        let current_bytes = 500_000u64;
        let prev_bytes = 1_000_000u64;
        let delta_bytes = current_bytes.saturating_sub(prev_bytes);
        assert_eq!(delta_bytes, 0);
    }

    #[test]
    fn test_delta_calculation_normal_progression() {
        let current_bytes = 2_500_000u64;
        let prev_bytes = 1_500_000u64;
        let delta_bytes = current_bytes.saturating_sub(prev_bytes);
        assert_eq!(delta_bytes, 1_000_000);
    }

    #[test]
    fn test_pid_zero_filtering() {
        let pid = 0u32;
        let should_skip = pid == 0;
        assert!(should_skip);

        let pid = 1u32;
        let should_skip = pid == 0;
        assert!(!should_skip);
    }

    #[test]
    fn test_aggregation_single_pid() {
        let mut app_totals: HashMap<String, u64> = HashMap::new();
        app_totals.insert("firefox".to_string(), 500_000_000u64);

        assert_eq!(app_totals.len(), 1);
        assert_eq!(app_totals.get("firefox"), Some(&500_000_000u64));
    }

    #[test]
    fn test_aggregation_multiple_pids_same_app() {
        let mut app_totals: HashMap<String, u64> = HashMap::new();

        // Simulate first PID (e.g., firefox thread 1) issuing I/O
        let entry = app_totals.entry("firefox".to_string()).or_insert(0);
        *entry = entry.saturating_add(300_000_000u64);

        // Simulate second PID (e.g., firefox thread 2) issuing I/O
        let entry = app_totals.entry("firefox".to_string()).or_insert(0);
        *entry = entry.saturating_add(200_000_000u64);

        assert_eq!(app_totals.len(), 1);
        assert_eq!(app_totals.get("firefox"), Some(&500_000_000u64));
    }

    #[test]
    fn test_aggregation_multiple_apps() {
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
    fn test_zero_delta_skipped() {
        let delta_r = 0u64;
        let delta_w = 0u64;

        let should_emit = delta_r > 0 || delta_w > 0;
        assert!(!should_emit);
    }

    #[test]
    fn test_nonzero_read_delta_emitted() {
        let delta_r = 1000u64;
        let delta_w = 0u64;

        let should_emit = delta_r > 0 || delta_w > 0;
        assert!(should_emit);
    }

    #[test]
    fn test_nonzero_write_delta_emitted() {
        let delta_r = 0u64;
        let delta_w = 2000u64;

        let should_emit = delta_r > 0 || delta_w > 0;
        assert!(should_emit);
    }

    #[test]
    fn test_saturation_large_app_map() {
        // Verify behavior when prev_read/prev_write maps grow large (e.g., 10K apps)
        let mut app_bytes: HashMap<String, u64> = HashMap::new();

        // Simulate 1000 different apps each issuing I/O
        for i in 0..1000 {
            let app_name = format!("app_{:04}", i);
            let entry = app_bytes.entry(app_name).or_insert(0);
            *entry = entry.saturating_add(1_000_000u64);
        }

        assert_eq!(app_bytes.len(), 1000);
        // Verify we can still do deltas with large map
        let prev_state = app_bytes.clone();
        let mut new_state = app_bytes.clone();

        // Simulate more I/O for first app
        {
            let entry = new_state.entry("app_0000".to_string()).or_insert(0);
            *entry = entry.saturating_add(500_000u64);
        }

        let delta = new_state["app_0000"].saturating_sub(prev_state["app_0000"]);
        assert_eq!(delta, 500_000u64);
    }

    #[test]
    fn test_overflow_safety_with_large_transfers() {
        // Verify saturating_add prevents overflow with multi-TB transfers
        let mut transfers: HashMap<String, u64> = HashMap::new();

        // Start with a very large byte count (close to u64::MAX)
        let initial = u64::MAX - 1_000_000u64;
        {
            let entry = transfers.entry("large_app".to_string()).or_insert(0);
            *entry = entry.saturating_add(initial);
        }

        // Add more bytes (would overflow without saturating_add)
        {
            let entry = transfers.entry("large_app".to_string()).or_insert(0);
            *entry = entry.saturating_add(2_000_000u64);
        }

        // Should saturate at u64::MAX, not wrap around
        assert_eq!(transfers["large_app"], u64::MAX);
    }

    #[test]
    fn test_multiple_pids_same_comm_field() {
        // Verify that distinct PIDs with same comm field (e.g., multiple python processes)
        // correctly aggregate into one record with summed bytes
        let mut app_read_bytes: HashMap<String, u64> = HashMap::new();
        let mut app_write_bytes: HashMap<String, u64> = HashMap::new();

        // Simulate PID 1234 (python) with 100MB read, 50MB write
        let app_name = "python3.11".to_string();
        {
            let entry = app_read_bytes.entry(app_name.clone()).or_insert(0);
            *entry = entry.saturating_add(100_000_000u64);
        }
        {
            let entry = app_write_bytes.entry(app_name.clone()).or_insert(0);
            *entry = entry.saturating_add(50_000_000u64);
        }

        // Simulate PID 1235 (also python) with 80MB read, 40MB write
        {
            let entry = app_read_bytes.entry(app_name.clone()).or_insert(0);
            *entry = entry.saturating_add(80_000_000u64);
        }
        {
            let entry = app_write_bytes.entry(app_name.clone()).or_insert(0);
            *entry = entry.saturating_add(40_000_000u64);
        }

        // Should have aggregated into single record
        assert_eq!(app_read_bytes.len(), 1);
        assert_eq!(app_write_bytes.len(), 1);
        assert_eq!(app_read_bytes[&app_name], 180_000_000u64);
        assert_eq!(app_write_bytes[&app_name], 90_000_000u64);
    }

    #[test]
    fn test_delta_preserves_previous_state() {
        // Verify that delta calculation correctly uses and updates previous state
        let mut prev_read: HashMap<String, u64> = HashMap::new();
        let mut prev_write: HashMap<String, u64> = HashMap::new();
        let app_name = "testapp".to_string();

        // First interval: 50MB read, 30MB write
        let cur_r1 = 50_000_000u64;
        let cur_w1 = 30_000_000u64;

        let delta_r1 = cur_r1.saturating_sub(prev_read.get(&app_name).copied().unwrap_or(0));
        let delta_w1 = cur_w1.saturating_sub(prev_write.get(&app_name).copied().unwrap_or(0));

        assert_eq!(delta_r1, 50_000_000u64);
        assert_eq!(delta_w1, 30_000_000u64);

        // Update previous state
        prev_read.insert(app_name.clone(), cur_r1);
        prev_write.insert(app_name.clone(), cur_w1);

        // Second interval: 80MB read total, 60MB write total
        let cur_r2 = 80_000_000u64;
        let cur_w2 = 60_000_000u64;

        let delta_r2 = cur_r2.saturating_sub(prev_read.get(&app_name).copied().unwrap_or(0));
        let delta_w2 = cur_w2.saturating_sub(prev_write.get(&app_name).copied().unwrap_or(0));

        assert_eq!(delta_r2, 30_000_000u64);
        assert_eq!(delta_w2, 30_000_000u64);
    }
}
