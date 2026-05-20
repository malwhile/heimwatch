//! Platform-specific power collectors.
//!
//! This module selects the appropriate power collector based on the target OS.
//! Currently only Linux is implemented with polling-based battery/AC state monitoring.
//! macOS and Windows have stub implementations for future development.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

// Export the appropriate collector for the current platform
#[cfg(target_os = "linux")]
pub use linux::{POLL_INTERVAL, PowerCollector};
#[cfg(target_os = "macos")]
pub use macos::{POLL_INTERVAL, PowerCollector};
#[cfg(target_os = "windows")]
pub use windows::{POLL_INTERVAL, PowerCollector};
