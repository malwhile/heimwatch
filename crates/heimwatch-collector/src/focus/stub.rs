//! Stub focus collector for non-Linux platforms.

use anyhow::Result;

/// Placeholder collector for macOS, Windows, etc.
pub struct FocusCollector;

impl FocusCollector {
    /// Non-Linux platforms do not support focus tracking (yet).
    pub fn try_new() -> Option<Self> {
        None
    }

    pub async fn run(
        &self,
        _storage: Arc<StorageLayer>,
        mut _shutdown: watch::Receiver<bool>,
    ) -> Result<()> {
        anyhow::bail!("Not yet implemented")
    }
}
