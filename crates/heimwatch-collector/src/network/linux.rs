//! Linux network collector using eBPF for kernel-space monitoring.
//!
//! Requires: CAP_BPF + CAP_PERFMON or root (Linux 5.8+)
//! Uses: aya framework for eBPF program loading and kprobe attachment

use crate::error::CollectorError;
use anyhow::Result;
use std::collections::HashMap;

use aya::Ebpf;
use aya::programs::KProbe;
use heimwatch_core::{MetricPayload, MetricRecord, NetworkData, current_unix_timestamp};
use heimwatch_ebpf_common::PidNetStats;

/// Embedded BPF object, compiled by build.rs at build time.
static BPF_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/heimwatch-ebpf"));

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
    /// - Reads all (pid, PidNetStats) pairs from the BPF map.
    /// - Resolves each PID to an app name via /proc.
    /// - Aggregates stats by app_name (handles multiple PIDs per app).
    /// - Subtracts previous snapshot to get per-interval deltas.
    /// - If current < previous for an app, the PID was reused — treat as 0 delta.
    pub fn collect_network(&mut self) -> Result<Vec<MetricRecord>> {
        let timestamp = current_unix_timestamp()?;

        // Get the NETWORK_STATS map from the BPF program
        // Note: For now, we read it as raw bytes and parse manually
        // TODO: Use typed HashMap once we can resolve the orphan rule properly
        let _map_ref = self
            .bpf
            .map("NETWORK_STATS")
            .ok_or_else(|| CollectorError::MapNotFound("NETWORK_STATS".to_string()))?;

        // Aggregate current totals by app name (multiple PIDs → same app)
        let current_by_app: HashMap<String, PidNetStats> = HashMap::new();

        // For now, create an empty map (placeholder)
        // In a real implementation, we would iterate the BPF map here
        // This is a TODO until we resolve the typed map access

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
}
