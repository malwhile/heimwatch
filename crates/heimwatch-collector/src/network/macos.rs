//! macOS network collector stub.
//!
//! macOS uses different system tracing mechanisms (DTrace, FSEvents, etc.)
//! which are not yet implemented. This is a placeholder for future work.

use heimwatch_core::{Collector, MetricRecord};
use std::error::Error;

/// Stub NetworkCollector for macOS.
/// Not yet implemented; returns empty results.
pub struct NetworkCollector;

impl NetworkCollector {
    /// Create a new NetworkCollector for macOS.
    /// Currently unimplemented; use DTrace or system call tracing in the future.
    pub fn new() -> Result<Self, Box<dyn Error>> {
        Err("Network monitoring not yet implemented on macOS".into())
    }

    /// Collect network metrics (stub).
    pub fn collect(&mut self) -> Result<Vec<MetricRecord>, Box<dyn Error>> {
        Ok(Vec::new())
    }
}

impl Collector for NetworkCollector {
    fn collect_network(&mut self) -> Result<Vec<MetricRecord>, Box<dyn std::error::Error>> {
        self.collect()
    }

    fn collect_power(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn collect_focus(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn collect_system_metrics(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}
