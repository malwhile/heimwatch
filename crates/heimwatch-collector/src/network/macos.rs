//! macOS network collector stub.
//!
//! macOS uses different system tracing mechanisms (DTrace, FSEvents, etc.)
//! which are not yet implemented. This is a placeholder for future work.

use crate::error::CollectorError;
use anyhow::Result;
use heimwatch_core::{CollectorEvent, MetricRecord};
use std::time::Duration;
use tokio::sync::{mpsc, watch};

/// Stub NetworkCollector for macOS.
/// Not yet implemented; returns empty results.
pub struct NetworkCollector;

impl NetworkCollector {
    /// Create a new NetworkCollector for macOS.
    /// Currently unimplemented; use DTrace or system call tracing in the future.
    pub fn new() -> Result<Self> {
        Err(anyhow::anyhow!(CollectorError::PlatformNotSupported(
            "macOS network collection not yet implemented".to_string()
        )))
    }

    /// Collect network metrics (stub).
    pub fn collect_network(&mut self) -> Result<Vec<MetricRecord>> {
        Ok(Vec::new())
    }

    /// Run the network collection loop (stub).
    pub async fn run(
        self,
        _tx: mpsc::Sender<CollectorEvent>,
        _shutdown: watch::Receiver<bool>,
        _interval: Duration,
    ) -> Result<()> {
        Ok(())
    }
}
