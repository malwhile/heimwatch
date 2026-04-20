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
        Err(CollectorError::PlatformNotSupported("macOS".to_string()).into())
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
