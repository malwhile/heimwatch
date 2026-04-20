//! Heimwatch collector: unified facade for platform-specific metric collectors.
//!
//! This crate provides a single `PlatformCollector` that abstracts away OS-specific
//! implementation details. Each metric type (network, power, focus, system) can have
//! platform-specific collectors that `PlatformCollector` coordinates.

pub mod error;
pub mod focus;
pub mod network;

use anyhow::Result;
pub use error::CollectorError;
pub use focus::FocusCollector;
pub use network::NetworkCollector;

/// Unified collector that delegates to platform-specific collectors.
///
/// This struct is a factory for platform-specific collectors. The daemon extracts
/// each collector and runs them as independent async/blocking tasks, sending
/// CollectorEvents through a shared channel.
pub struct PlatformCollector {
    network: Option<NetworkCollector>,
    focus_collector: Option<FocusCollector>,
}

impl PlatformCollector {
    /// Initialize the platform collector with OS-specific implementations.
    pub fn new() -> Result<Self> {
        let network = NetworkCollector::new().ok();
        let focus_collector = FocusCollector::try_new();

        Ok(PlatformCollector {
            network,
            focus_collector,
        })
    }

    /// Extracts the focus collector for spawning as an independent task.
    ///
    /// This is used by the daemon to run focus tracking in a separate tokio task
    /// (since focus is event-driven, not poll-based like the network collector).
    pub fn take_focus_collector(&mut self) -> Option<FocusCollector> {
        self.focus_collector.take()
    }

    /// Extracts the network collector for spawning as an independent blocking task.
    ///
    /// This is used by the daemon to run network collection in a separate spawn_blocking task
    /// (since network collection uses eBPF which is not Send on Linux).
    pub fn take_network_collector(&mut self) -> Option<NetworkCollector> {
        self.network.take()
    }
}
