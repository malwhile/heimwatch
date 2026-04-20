//! Collector error types.

use thiserror::Error;

/// Errors that can occur in collector operations.
#[derive(Debug, Error)]
pub enum CollectorError {
    /// eBPF program not found in loaded object.
    #[error("eBPF program not found: {0}")]
    ProgramNotFound(String),

    /// eBPF program type mismatch (expected kprobe but found different type).
    #[error("eBPF program type mismatch: {0}")]
    ProgramTypeMismatch(String),

    /// eBPF map not found in loaded object.
    #[error("eBPF map not found: {0}")]
    MapNotFound(String),

    /// Underlying eBPF (aya) error.
    #[cfg(target_os = "linux")]
    #[error("eBPF error: {0}")]
    EbpfError(String),

    /// Network monitoring not yet implemented for this platform.
    #[error("network monitoring not yet implemented on {0}")]
    PlatformNotSupported(String),

    /// System time error (e.g., clock not available).
    #[error("system time error")]
    SystemTimeError,

    /// Wayland wlr-foreign-toplevel protocol unavailable.
    #[error("wlr-foreign-toplevel protocol unavailable")]
    WlrToplevelUnavailable,

    /// D-Bus connection failed (GNOME fallback).
    #[error("D-Bus connection failed: {0}")]
    DbusError(String),

    /// Focus tracking unavailable on this compositor.
    #[error("focus tracking unavailable: no supported compositor found")]
    FocusUnavailable,

    /// Network collector initialization failed (e.g., missing eBPF capabilities).
    #[error("network collector initialization failed: {0}")]
    NetworkInitializationFailed(String),

    /// CPU collector initialization failed (e.g., missing eBPF capabilities).
    #[error("CPU collector initialization failed: {0}")]
    CpuInitializationFailed(String),

    /// Disk collector initialization failed (e.g., missing eBPF capabilities).
    #[error("disk collector initialization failed: {0}")]
    DiskInitializationFailed(String),
}

#[cfg(target_os = "linux")]
impl From<aya::EbpfError> for CollectorError {
    fn from(err: aya::EbpfError) -> Self {
        CollectorError::EbpfError(err.to_string())
    }
}

impl From<std::time::SystemTimeError> for CollectorError {
    fn from(_: std::time::SystemTimeError) -> Self {
        CollectorError::SystemTimeError
    }
}
