use std::collections::HashMap;
use std::error::Error;
use std::time::{SystemTime, UNIX_EPOCH};

use aya::Ebpf;
use aya::programs::KProbe;
use heimwatch_core::{Collector, MetricPayload, MetricRecord, NetworkData};
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

impl NetworkCollector {
    /// Load and attach the BPF kprobes. Requires CAP_BPF or root.
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let mut bpf = Ebpf::load(BPF_BYTES)?;

        // Attach kprobe to tcp_sendmsg (tx)
        let sendmsg_prog: &mut KProbe = bpf
            .program_mut("trace_sendmsg")
            .ok_or("trace_sendmsg not found")?
            .try_into()
            .map_err(|_| "trace_sendmsg is not a kprobe")?;
        sendmsg_prog.load()?;
        sendmsg_prog.attach("tcp_sendmsg", 0)?;

        // Attach kretprobe to tcp_recvmsg (rx)
        let recvmsg_prog: &mut KProbe = bpf
            .program_mut("trace_recvmsg")
            .ok_or("trace_recvmsg not found")?
            .try_into()
            .map_err(|_| "trace_recvmsg is not a kprobe")?;
        recvmsg_prog.load()?;
        recvmsg_prog.attach("tcp_recvmsg", 0)?;

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
    pub fn collect(&mut self) -> Result<Vec<MetricRecord>, Box<dyn Error>> {
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

        // Get the NETWORK_STATS map from the BPF program
        // Note: For now, we read it as raw bytes and parse manually
        // TODO: Use typed HashMap once we can resolve the orphan rule properly
        let _map_ref = self
            .bpf
            .map("NETWORK_STATS")
            .ok_or("NETWORK_STATS map not found")?;

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

impl Collector for NetworkCollector {
    fn collect_network(&mut self) -> Result<Vec<MetricRecord>, Box<dyn std::error::Error>> {
        self.collect()
    }

    fn collect_power(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn collect_focus(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn collect_system_metrics(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}
