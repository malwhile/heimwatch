//! Stub CPU collector for non-Linux platforms.

/// Poll interval for CPU usage collection (5 seconds).
pub const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

use anyhow::Result;
use tokio::sync::{mpsc, watch};

use heimwatch_core::CollectorEvent;

/// Placeholder collector for macOS, Windows, etc.
pub struct CpuCollector;

impl CpuCollector {
    /// Non-Linux platforms do not support CPU eBPF tracking (yet).
    pub fn new() -> Result<Self> {
        Err(anyhow::anyhow!(
            "CPU tracking not yet implemented on this platform"
        ))
    }

    pub async fn run(
        self,
        _tx: mpsc::Sender<CollectorEvent>,
        mut _shutdown: watch::Receiver<bool>,
    ) -> Result<()> {
        anyhow::bail!("CPU tracking not yet implemented on this platform")
    }
}
