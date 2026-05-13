//! Linux network collector using eBPF for kernel-space monitoring.
//!
//! Requires: CAP_BPF + CAP_PERFMON or root (Linux 5.8+)
//! Uses: aya framework for eBPF program loading and kprobe attachment

/// Poll interval for network traffic collection (5 seconds).
pub const POLL_INTERVAL: Duration = Duration::from_secs(5);

use crate::error::CollectorError;
use anyhow::Result;
use std::collections::HashMap;

use aya::Ebpf;
use aya::maps::HashMap as AyaHashMap;
use aya::programs::KProbe;
use heimwatch_core::{
    CollectorEvent, MetricPayload, MetricRecord, NetworkData, current_unix_timestamp,
};
use heimwatch_ebpf_common::PidNetStats;
use std::time::Duration;
use tokio::sync::{mpsc, watch};

/// Local Pod-compatible mirror of PidNetStats.
///
/// The orphan rule prevents implementing `aya::Pod` for `PidNetStats` in this crate
/// (since both `aya` and `heimwatch-ebpf-common` are external). This mirror type has
/// identical layout (both are `#[repr(C)]`) and can be safely converted.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LocalPidNetStats {
    tx_bytes: u64,
    rx_bytes: u64,
    comm: [u8; 16], // process name captured in kernel-space
}

// Safety: repr(C), all fields (u64, u64, [u8; 16]) are valid for all bit patterns, no padding.
unsafe impl aya::Pod for LocalPidNetStats {}

/// Embedded BPF object, compiled by build.rs at build time.
static BPF_BYTES: &[u8] = aya::include_bytes_aligned!(concat!(env!("OUT_DIR"), "/heimwatch-ebpf"));

/// Owns the loaded BPF program and maintains delta state.
pub struct NetworkCollector {
    /// The loaded eBPF object (owns program fds).
    bpf: Ebpf,
    /// Previous snapshot: app_name -> PidNetStats totals.
    /// Keyed by app_name (not PID) to survive PID reuse.
    prev_state: HashMap<String, PidNetStats>,
}

/// Load and attach a kprobe program.
fn load_and_attach_kprobe(bpf: &mut Ebpf, prog_name: &str, kernel_func: &str) -> Result<()> {
    let prog: &mut KProbe = bpf
        .program_mut(prog_name)
        .ok_or_else(|| CollectorError::ProgramNotFound(prog_name.to_string()))?
        .try_into()
        .map_err(|_| CollectorError::ProgramTypeMismatch(prog_name.to_string()))?;
    prog.load()?;
    prog.attach(kernel_func, 0)?;
    Ok(())
}

impl NetworkCollector {
    /// Load and attach the BPF kprobes. Requires CAP_BPF or root.
    pub fn new() -> Result<Self> {
        let mut bpf = Ebpf::load(BPF_BYTES)?;

        // Attach kprobe to tcp_sendmsg (tx)
        load_and_attach_kprobe(&mut bpf, "trace_sendmsg", "tcp_sendmsg")?;

        // Attach kretprobe to tcp_recvmsg (rx)
        load_and_attach_kprobe(&mut bpf, "trace_recvmsg", "tcp_recvmsg")?;

        Ok(NetworkCollector {
            bpf,
            prev_state: HashMap::new(),
        })
    }

    /// Poll the BPF map once. Returns one MetricRecord per active app.
    ///
    /// Delta calculation:
    /// - Reads all (process_name, PidNetStats) pairs from the BPF map.
    /// - Process name (comm field) is the key, so aggregation happens automatically in eBPF.
    /// - Subtracts previous snapshot to get per-interval deltas.
    /// - If current < previous for an app, treat as 0 delta.
    pub fn collect_network(&mut self) -> Result<Vec<MetricRecord>> {
        let timestamp = current_unix_timestamp()?;

        // Get the NETWORK_STATS map from the BPF program and iterate it
        let map_ref = self
            .bpf
            .map_mut("NETWORK_STATS")
            .ok_or_else(|| CollectorError::MapNotFound("NETWORK_STATS".to_string()))?;

        // Map is now keyed by process name ([u8; 16]), not PID
        let stats_map: AyaHashMap<_, [u8; 16], LocalPidNetStats> = AyaHashMap::try_from(map_ref)?;

        // Process names are keys; no need to aggregate further
        let mut current_by_app: HashMap<String, PidNetStats> = HashMap::new();

        for entry in stats_map.iter() {
            let (comm, local_stats) = entry?;

            // Convert process name to string (null-terminated)
            let app_name = comm_to_string(&comm).unwrap_or_else(|| "(unknown)".to_string());

            current_by_app.insert(
                app_name,
                PidNetStats {
                    tx_bytes: local_stats.tx_bytes,
                    rx_bytes: local_stats.rx_bytes,
                    comm,
                },
            );
        }

        // Calculate deltas
        let mut records = Vec::new();
        for (app_name, current) in &current_by_app {
            let prev = self.prev_state.get(app_name).copied().unwrap_or_default();

            // If current < prev, PID was reused — delta is 0 for this interval
            let tx_delta = current.tx_bytes.saturating_sub(prev.tx_bytes);
            let rx_delta = current.rx_bytes.saturating_sub(prev.rx_bytes);

            // Only emit a record if there was actual traffic
            if tx_delta > 0 || rx_delta > 0 {
                records.push(MetricRecord {
                    app_name: app_name.clone(),
                    timestamp,
                    payload: MetricPayload::Net(NetworkData {
                        tx_bytes: tx_delta,
                        rx_bytes: rx_delta,
                        connections: 0,
                    }),
                });
            }
        }

        // Update previous state
        self.prev_state = current_by_app;

        Ok(records)
    }

    /// Run the network collection loop.
    ///
    /// Polls for network metrics at the configured interval using tokio::select!
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
                    // Collect network metrics
                    for record in self.collect_network()? {
                        if let MetricPayload::Net(_) = &record.payload {
                            let event = CollectorEvent {
                                app_name: record.app_name.clone(),
                                payload: record.payload,
                                timestamp: record.timestamp,
                            };
                            if let Err(e) = tx.send(event).await {
                                log::error!(
                                    "Failed to send network event for app '{}': {}",
                                    record.app_name,
                                    e
                                );
                            }
                        }
                    }
                }

                _ = shutdown.changed() => {
                    log::debug!("Network collector received shutdown signal");
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
