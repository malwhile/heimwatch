//! macOS memory collector stub.

use std::time::Duration;

pub const POLL_INTERVAL: Duration = Duration::from_secs(10);

pub struct MemoryCollector;

impl MemoryCollector {
    pub fn new() -> anyhow::Result<Self> {
        Err(anyhow::anyhow!("not yet implemented on this platform"))
    }

    pub async fn run(
        self,
        _tx: tokio::sync::mpsc::Sender<heimwatch_core::CollectorEvent>,
        _shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}
