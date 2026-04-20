//! Windows network collector stub.
//!
//! Windows uses different system tracing mechanisms (ETW, WMI, etc.)
//! which are not yet implemented. This is a placeholder for future work.

use crate::error::CollectorError;
use anyhow::Result;
use heimwatch_core::{CollectorEvent, MetricRecord};
use std::time::Duration;
use tokio::sync::{mpsc, watch};

/// Stub NetworkCollector for Windows.
/// Not yet implemented; returns empty results.
pub struct NetworkCollector;

impl NetworkCollector {
    /// Create a new NetworkCollector for Windows.
    /// Currently unimplemented; use ETW or WMI in the future.
    pub fn new() -> Result<Self> {
        Err(anyhow::anyhow!(
            CollectorError::PlatformNotSupported("Windows network collection not yet implemented".to_string())
        ))
    }

    /// Collect network metrics (stub).
    pub fn collect_network(&mut self) -> Result<Vec<MetricRecord>> {
        Ok(Vec::new())
    }

    /// Run the network collection loop (stub).
    pub fn run(
        self,
        _tx: mpsc::Sender<CollectorEvent>,
        _shutdown: watch::Receiver<bool>,
        _interval: Duration,
    ) -> Result<()> {
        Ok(())
    }
}
