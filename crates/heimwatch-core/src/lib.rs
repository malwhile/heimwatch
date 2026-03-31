// crates/heimwatch-core/src/lib.rs

/// OS Abstraction Trait for Data Collection
/// Implement this for each target platform (Linux, macOS, Windows, BSD)
pub trait Collector {
    fn collect_network(&self) -> Result<(), Box<dyn std::error::Error>>;
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
