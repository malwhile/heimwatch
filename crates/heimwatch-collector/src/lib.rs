//! Heimwatch collector: unified facade for platform-specific metric collectors.
//!
//! This crate provides a single `PlatformCollector` that abstracts away OS-specific
//! implementation details. Each metric type (network, power, focus, system) can have
//! platform-specific collectors that `PlatformCollector` coordinates.

pub mod cpu;
pub mod disk;
pub mod error;
pub mod focus;
pub mod gpu;
pub mod memory;
pub mod network;
pub mod power;
pub mod util;

use anyhow::Result;
pub use cpu::CpuCollector;
pub use disk::DiskCollector;
pub use error::CollectorError;
pub use focus::FocusCollector;
pub use gpu::GpuCollector;
pub use memory::MemoryCollector;
pub use network::NetworkCollector;
pub use power::PowerCollector;

/// Unified collector that delegates to platform-specific collectors.
///
/// This struct is a factory for platform-specific collectors. The daemon extracts
/// each collector and runs them as independent async/blocking tasks, sending
/// CollectorEvents through a shared channel.
pub struct PlatformCollector {
    network: Option<NetworkCollector>,
    focus_collector: Option<FocusCollector>,
    cpu: Option<CpuCollector>,
    disk: Option<DiskCollector>,
    memory: Option<MemoryCollector>,
    gpu: Option<GpuCollector>,
    power: Option<PowerCollector>,
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

        let disk = match DiskCollector::new() {
            Ok(dc) => Some(dc),
            Err(e) => {
                if e.to_string().contains("not yet implemented") {
                    log::debug!("Disk collection not available: {}", e);
                } else {
                    log::warn!("Disk collector initialization failed: {}", e);
                }
                None
            }
        };

        let memory = match MemoryCollector::new() {
            Ok(mc) => Some(mc),
            Err(e) => {
                if e.to_string().contains("not yet implemented") {
                    log::debug!("Memory collection not available: {}", e);
                } else {
                    log::warn!("Memory collector initialization failed: {}", e);
                }
                None
            }
        };

        let gpu = match GpuCollector::new() {
            Ok(gc) => Some(gc),
            Err(e) => {
                if e.to_string().contains("not yet implemented") {
                    log::debug!("GPU collection not available: {}", e);
                } else {
                    log::warn!("GPU collector initialization failed: {}", e);
                }
                None
            }
        };

        let power = match PowerCollector::new() {
            Ok(pc) => Some(pc),
            Err(e) => {
                if e.to_string().contains("not yet implemented") {
                    log::debug!("Power collection not available: {}", e);
                } else {
                    log::warn!("Power collector initialization failed: {}", e);
                }
                None
            }
        };

        Ok(PlatformCollector {
            network,
            focus_collector,
            cpu,
            disk,
            memory,
            gpu,
            power,
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

    /// Extracts the disk collector for spawning as an independent task.
    ///
    /// This is used by the daemon to run disk collection in a separate tokio task.
    pub fn take_disk_collector(&mut self) -> Option<DiskCollector> {
        self.disk.take()
    }

    /// Extracts the memory collector for spawning as an independent task.
    ///
    /// This is used by the daemon to run memory collection in a separate tokio task.
    pub fn take_memory_collector(&mut self) -> Option<MemoryCollector> {
        self.memory.take()
    }

    /// Extracts the GPU collector for spawning as an independent task.
    ///
    /// This is used by the daemon to run GPU collection in a separate tokio task.
    pub fn take_gpu_collector(&mut self) -> Option<GpuCollector> {
        self.gpu.take()
    }

    /// Extracts the power collector for spawning as an independent task.
    ///
    /// This is used by the daemon to run power collection in a separate tokio task.
    pub fn take_power_collector(&mut self) -> Option<PowerCollector> {
        self.power.take()
    }
}
