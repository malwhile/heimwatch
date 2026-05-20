//! macOS power collector stub.
//!
//! Power collection is not yet implemented on macOS.

use anyhow::Result;
use std::time::Duration;
use tokio::sync::{mpsc, watch};

#[allow(unused_imports)]
use heimwatch_core::{CollectorEvent, MetricPayload};

pub const POLL_INTERVAL: Duration = Duration::from_secs(30);

pub struct PowerCollector;

impl PowerCollector {
    pub fn new() -> Result<Self> {
        Err(anyhow::anyhow!(
            "Power collection not yet implemented on macOS"
        ))
    }

    pub async fn run(
        self,
        _tx: mpsc::Sender<CollectorEvent>,
        _shutdown: watch::Receiver<bool>,
    ) -> Result<()> {
        Ok(())
    }
}
