//! Heimwatch collector: unified facade for platform-specific metric collectors.
//!
//! This crate provides a single `PlatformCollector` that abstracts away OS-specific
//! implementation details. Each metric type (network, power, focus, system) can have
//! platform-specific collectors that `PlatformCollector` coordinates.

pub mod error;
mod network;

use anyhow::Result;
pub use error::CollectorError;
use heimwatch_core::{Collector, MetricRecord};
use network::NetworkCollector;

/// Unified collector that delegates to platform-specific collectors.
///
/// This struct implements the `Collector` trait and coordinates data collection
/// across different metric types (network, power, focus, system metrics).
/// Platform-specific implementations are selected at compile time.
pub struct PlatformCollector {
    network: NetworkCollector,
}

impl PlatformCollector {
    /// Initialize the platform collector with OS-specific implementations.
    pub fn new() -> Result<Self> {
        let network = NetworkCollector::new()?;

        Ok(PlatformCollector { network })
    }
}

impl Collector for PlatformCollector {
    fn collect_network(&mut self) -> Result<Vec<MetricRecord>> {
        self.network.collect_network()
    }
}
