//! Platform-specific disk I/O collectors.
//!
//! This module selects the appropriate disk collector based on the target OS.
//! Currently only Linux is fully implemented with eBPF-based block_rq_issue monitoring.
//! macOS and Windows have stub implementations for future development.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

// Export the appropriate collector for the current platform
#[cfg(target_os = "linux")]
pub use linux::{DiskCollector, POLL_INTERVAL};
#[cfg(target_os = "macos")]
pub use macos::{DiskCollector, POLL_INTERVAL};
#[cfg(target_os = "windows")]
pub use windows::{DiskCollector, POLL_INTERVAL};
