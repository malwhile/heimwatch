//! Stub focus collector for non-Linux platforms.

/// Placeholder collector for macOS, Windows, etc.
pub struct FocusCollector;

impl FocusCollector {
    /// Non-Linux platforms do not support focus tracking (yet).
    pub fn try_new() -> Option<Self> {
        None
    }
}
