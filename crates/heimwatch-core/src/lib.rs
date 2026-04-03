// crates/heimwatch-core/src/lib.rs

pub mod metrics;
pub mod process;
use std::time::{SystemTime, UNIX_EPOCH};

pub use metrics::*;

use anyhow::{Context as _, Result};

/// OS Abstraction Trait for Data Collection
/// Implement this for each target platform (Linux, macOS, Windows, BSD)
pub trait Collector {
    /// Collect network traffic metrics. Returns one MetricRecord per active application.
    /// Requires &mut self because implementations may maintain mutable state (e.g., delta tracking).
    fn collect_network(&mut self) -> Result<Vec<MetricRecord>>;

    /// Collect power consumption metrics. Default: unimplemented (returns Ok(())).
    fn collect_power(&self) -> Result<()> {
        Ok(())
    }

    /// Collect focus time metrics. Default: unimplemented (returns Ok(())).
    fn collect_focus(&self) -> Result<()> {
        Ok(())
    }

    /// Collect system metrics (CPU, memory, disk, GPU). Default: unimplemented (returns Ok(())).
    fn collect_system_metrics(&self) -> Result<()> {
        Ok(())
    }
}

/// OS Abstraction Trait for Service Management
pub trait ServiceManager {
    fn start_service(&self) -> Result<()>;
    fn stop_service(&self) -> Result<()>;
    fn is_running(&self) -> bool;
}

pub fn current_unix_timestamp() -> anyhow::Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .context("Failed to get current timestamp")
}
