//! Platform-specific CPU collectors.
//!
//! This module selects the appropriate CPU collector based on the target OS.
//! Currently only Linux is fully implemented with eBPF-based sched_switch monitoring.

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::{CpuCollector, POLL_INTERVAL};

#[cfg(not(target_os = "linux"))]
mod stub;

#[cfg(not(target_os = "linux"))]
pub use stub::CpuCollector;
