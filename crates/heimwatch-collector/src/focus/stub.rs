//! Stub focus collector for non-Linux platforms.

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
