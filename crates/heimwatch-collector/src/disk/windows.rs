//! Windows disk I/O collector stub.
//!
//! Windows uses Event Tracing for Windows (ETW) and Performance Counters
//! which are not yet implemented. This is a placeholder for future work.

use anyhow::Result;
use std::time::Duration;
use tokio::sync::{mpsc, watch};

use heimwatch_core::{CollectorEvent, MetricRecord};

/// Poll interval for disk I/O collection (5 seconds).
pub const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Stub DiskCollector for Windows.
/// Not yet implemented; returns empty results.
pub struct DiskCollector;

impl DiskCollector {
    /// Create a new DiskCollector for Windows.
    /// Currently unimplemented; use ETW or Performance Counters in the future.
    pub fn new() -> Result<Self> {
        log::warn!("Disk tracking not yet implemented on Windows");
        Err(anyhow::anyhow!(
            "Disk tracking not yet implemented on Windows"
        ))
    }

    /// Collect disk metrics (stub).
    pub fn collect_disk(&mut self, _interval: Duration) -> Result<Vec<MetricRecord>> {
        log::debug!("Disk collection not available on Windows");
        Ok(Vec::new())
    }

    /// Run the disk collection loop (stub).
    pub async fn run(
        self,
        _tx: mpsc::Sender<CollectorEvent>,
        _shutdown: watch::Receiver<bool>,
    ) -> Result<()> {
        log::warn!("Disk tracking not implemented on Windows");
        anyhow::bail!("Disk tracking not yet implemented on Windows")
    }
}
