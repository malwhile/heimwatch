//! Stub GPU collector for non-Linux platforms.

use anyhow::Result;
use heimwatch_core::CollectorEvent;
use std::time::Duration;
use tokio::sync::{mpsc, watch};

pub struct GpuCollector;

impl GpuCollector {
    pub fn new() -> Result<Self> {
        Err(anyhow::anyhow!(
            "GPU collection not yet implemented on this platform"
        ))
    }

    pub async fn run(
        self,
        _tx: mpsc::Sender<CollectorEvent>,
        mut _shutdown: watch::Receiver<bool>,
    ) -> Result<()> {
        unreachable!("GPU collector cannot be created on non-Linux platforms")
    }

    pub fn collect_gpus(
        &mut self,
        _interval: Duration,
    ) -> Result<Vec<heimwatch_core::MetricRecord>> {
        unreachable!("GPU collector cannot be created on non-Linux platforms")
    }
}
