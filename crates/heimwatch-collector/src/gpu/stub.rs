//! Stub GPU collector for non-Linux platforms.

use anyhow::Result;

pub struct GpuCollector;

impl GpuCollector {
    pub fn new() -> Result<Self> {
        Err(anyhow::anyhow!("GPU collection not yet implemented on this platform"))
    }

    pub async fn run(
        self,
        _tx: tokio::sync::mpsc::Sender<crate::util::CollectorEvent>,
        mut _shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> Result<()> {
        unreachable!("GPU collector cannot be created on non-Linux platforms")
    }
}
