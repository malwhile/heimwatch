//! Heimwatch collector: unified facade for platform-specific metric collectors.
//!
//! This crate provides a single `PlatformCollector` that abstracts away OS-specific
//! implementation details. Each metric type (network, power, focus, system) can have
//! platform-specific collectors that `PlatformCollector` coordinates.

pub mod cpu;
pub mod error;
pub mod focus;
pub mod network;

use anyhow::Result;
pub use cpu::CpuCollector;
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
    cpu: Option<CpuCollector>,
}

impl PlatformCollector {
    /// Initialize the platform collector with OS-specific implementations.
    pub fn new() -> Result<Self> {
        let network = match NetworkCollector::new() {
            Ok(nc) => Some(nc),
            Err(e) => {
                // Log the error to distinguish between platform unavailability and initialization failure
                if e.to_string().contains("not yet implemented") {
                    log::debug!("Network collection not available: {}", e);
                } else {
                    log::warn!("Network collector initialization failed: {}", e);
                }
                None
            }
        };
        let focus_collector = FocusCollector::try_new();

        let cpu = match CpuCollector::new() {
            Ok(cc) => Some(cc),
            Err(e) => {
                if e.to_string().contains("not yet implemented") {
                    log::debug!("CPU collection not available: {}", e);
                } else {
                    log::warn!("CPU collector initialization failed: {}", e);
                }
                None
            }
        };

        Ok(PlatformCollector {
            network,
            focus_collector,
            cpu,
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

    /// Extracts the CPU collector for spawning as an independent task.
    ///
    /// This is used by the daemon to run CPU collection in a separate tokio task.
    pub fn take_cpu_collector(&mut self) -> Option<CpuCollector> {
        self.cpu.take()
    }
}
