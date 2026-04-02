// crates/heimwatch-core/src/lib.rs

pub mod metrics;
pub mod process;
pub use metrics::*;

/// OS Abstraction Trait for Data Collection
/// Implement this for each target platform (Linux, macOS, Windows, BSD)
pub trait Collector {
    /// Collect network traffic metrics. Returns one MetricRecord per active application.
    /// Requires &mut self because implementations may maintain mutable state (e.g., delta tracking).
    fn collect_network(&mut self) -> Result<Vec<MetricRecord>, Box<dyn std::error::Error>>;
    fn collect_power(&self) -> Result<(), Box<dyn std::error::Error>>;
    fn collect_focus(&self) -> Result<(), Box<dyn std::error::Error>>;
    fn collect_system_metrics(&self) -> Result<(), Box<dyn std::error::Error>>;
}

/// OS Abstraction Trait for Service Management
pub trait ServiceManager {
    fn start_service(&self) -> Result<(), Box<dyn std::error::Error>>;
    fn stop_service(&self) -> Result<(), Box<dyn std::error::Error>>;
    fn is_running(&self) -> bool;
}
