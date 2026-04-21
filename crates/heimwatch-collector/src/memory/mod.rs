//! Platform-specific memory collectors.
//!
//! This module selects the appropriate memory collector based on the target OS.
//! Currently only Linux is fully implemented with polling-based `/proc/status` monitoring.
//! macOS and Windows have stub implementations for future development.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

// Export the appropriate collector for the current platform
#[cfg(target_os = "linux")]
pub use linux::{MemoryCollector, POLL_INTERVAL};
#[cfg(target_os = "macos")]
pub use macos::{MemoryCollector, POLL_INTERVAL};
#[cfg(target_os = "windows")]
pub use windows::{MemoryCollector, POLL_INTERVAL};
