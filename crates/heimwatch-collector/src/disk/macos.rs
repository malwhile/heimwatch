//! macOS disk I/O collector stub.
//!
//! macOS uses different I/O tracing mechanisms (DTrace, etc.)
//! which are not yet implemented. This is a placeholder for future work.

use anyhow::Result;
use std::time::Duration;
use tokio::sync::{mpsc, watch};

use heimwatch_core::{CollectorEvent, MetricRecord};

/// Poll interval for disk I/O collection (5 seconds).
pub const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Stub DiskCollector for macOS.
/// Not yet implemented; returns empty results.
pub struct DiskCollector;

impl DiskCollector {
    /// Create a new DiskCollector for macOS.
    /// Currently unimplemented; use DTrace or system call tracing in the future.
    pub fn new() -> Result<Self> {
        log::warn!("Disk tracking not yet implemented on macOS");
        Err(anyhow::anyhow!(
            "Disk tracking not yet implemented on macOS"
        ))
    }

    /// Collect disk metrics (stub).
    pub fn collect_disk(&mut self, _interval: Duration) -> Result<Vec<MetricRecord>> {
        log::debug!("Disk collection not available on macOS");
        Ok(Vec::new())
    }

    /// Run the disk collection loop (stub).
    pub async fn run(
        self,
        _tx: mpsc::Sender<CollectorEvent>,
        _shutdown: watch::Receiver<bool>,
    ) -> Result<()> {
        log::warn!("Disk tracking not implemented on macOS");
        anyhow::bail!("Disk tracking not yet implemented on macOS")
    }
}
