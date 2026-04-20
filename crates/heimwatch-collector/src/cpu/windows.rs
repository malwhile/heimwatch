//! Windows CPU collector stub.
//!
//! Windows uses different system tracing mechanisms (ETW, etc.)
//! which are not yet implemented. This is a placeholder for future work.

use anyhow::Result;
use std::time::Duration;
use tokio::sync::{mpsc, watch};

use heimwatch_core::{CollectorEvent, MetricRecord};

/// Poll interval for CPU usage collection (5 seconds).
pub const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Stub CpuCollector for Windows.
/// Not yet implemented; returns empty results.
pub struct CpuCollector;

impl CpuCollector {
    /// Create a new CpuCollector for Windows.
    /// Currently unimplemented; use ETW or WMI in the future.
    pub fn new() -> Result<Self> {
        log::warn!("CPU tracking not yet implemented on Windows");
        Err(anyhow::anyhow!(
            "CPU tracking not yet implemented on Windows"
        ))
    }

    /// Collect CPU metrics (stub).
    pub fn collect_cpu(&mut self, _interval: Duration) -> Result<Vec<MetricRecord>> {
        log::debug!("CPU collection not available on Windows");
        Ok(Vec::new())
    }

    /// Run the CPU collection loop (stub).
    pub async fn run(
        self,
        _tx: mpsc::Sender<CollectorEvent>,
        _shutdown: watch::Receiver<bool>,
    ) -> Result<()> {
        log::warn!("CPU tracking not implemented on Windows");
        anyhow::bail!("CPU tracking not yet implemented on Windows")
    }
}
