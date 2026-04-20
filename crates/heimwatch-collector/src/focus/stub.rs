//! Stub focus collector for non-Linux platforms.

/// Event-driven collector (no polling interval needed). Kept for consistency with other collectors.
pub const _POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(0);

use anyhow::Result;
use tokio::sync::{mpsc, watch};

use heimwatch_core::CollectorEvent;

/// Placeholder collector for macOS, Windows, etc.
pub struct FocusCollector;

impl FocusCollector {
    /// Non-Linux platforms do not support focus tracking (yet).
    pub fn try_new() -> Option<Self> {
        None
    }

    pub async fn run(
        &self,
        _tx: mpsc::Sender<CollectorEvent>,
        mut _shutdown: watch::Receiver<bool>,
    ) -> Result<()> {
        anyhow::bail!("Not yet implemented")
    }
}
