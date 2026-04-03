//! Platform-specific network collectors.
//!
//! This module selects the appropriate network collector based on the target OS.
//! Currently only Linux is fully implemented with eBPF-based monitoring.
//! macOS and Windows have stub implementations for future development.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

// Export the appropriate collector for the current platform
#[cfg(target_os = "linux")]
pub use linux::NetworkCollector;
#[cfg(target_os = "macos")]
pub use macos::NetworkCollector;
#[cfg(target_os = "windows")]
pub use windows::NetworkCollector;
