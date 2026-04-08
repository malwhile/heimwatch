//! Wayland-based focus tracking on Linux.
//!
//! Listens to window focus changes via the wlr-foreign-toplevel-management-v1
//! protocol or falls back to GNOME D-Bus signals. Tracks elapsed time in memory
//! and persists focus sessions to the storage layer on focus-change events.

mod dbuslib;
mod wlrlib;

use anyhow::Result;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, watch};

use heimwatch_core::current_unix_timestamp;
use heimwatch_storage::StorageLayer;

/// Per-toplevel state accumulated between protocol events.
pub struct ToplevelInfo {
    pub app_id: Option<String>,
    pub is_activated: bool,
}

/// Represents the source of focus events.
enum FocusSource {
    WlrToplevel,
    GnomeDbus,
}

/// Collects focus time data from the Wayland compositor.
pub struct FocusCollector {
    source: FocusSource,
}

/// In-memory state tracking the currently focused application.
struct FocusState {
    current_app: Option<String>,
    focus_start: Instant,
}

impl FocusCollector {
    /// Attempts to create a focus collector.
    ///
    /// Returns `Some` if either the wlr-foreign-toplevel protocol or GNOME D-Bus
    /// is available. Returns `None` if neither is accessible (graceful degradation).
    pub fn try_new() -> Option<Self> {
        // Try Wayland first
        if wlrlib::try_wlr_toplevel_available().is_ok() {
            log::debug!("Focus tracking: wlr-foreign-toplevel-management-v1 available");
            return Some(FocusCollector {
                source: FocusSource::WlrToplevel,
            });
        }

        // Fall back to GNOME D-Bus
        if dbuslib::try_gnome_dbus_available().is_ok() {
            log::debug!("Focus tracking: GNOME D-Bus available (fallback)");
            return Some(FocusCollector {
                source: FocusSource::GnomeDbus,
            });
        }

        None
    }

    /// Runs the focus tracking event loop.
    ///
    /// Listens for focus-change events and persists elapsed time to storage.
    /// Respects the shutdown signal and flushes the current session on exit.
    pub async fn run(
        &self,
        storage: Arc<StorageLayer>,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<()> {
        let (event_tx, event_rx) = mpsc::channel::<Option<String>>(100);

        // Spawn the platform-specific listener
        let mut listener_handle = match &self.source {
            FocusSource::WlrToplevel => {
                let tx = event_tx.clone();
                tokio::task::spawn_blocking(move || wlrlib::spawn_wlr_toplevel_listener(tx))
            }
            FocusSource::GnomeDbus => {
                let tx = event_tx.clone();
                tokio::spawn(dbuslib::spawn_gnome_dbus_listener(tx))
            }
        };

        drop(event_tx); // Drop original sender so receiver knows when listener task ends

        let mut state = FocusState {
            current_app: None,
            focus_start: Instant::now(),
        };

        let mut event_rx = event_rx;

        loop {
            tokio::select! {
                Some(raw_id) = event_rx.recv() => {
                    let new_app = normalize_app_id(raw_id);

                    // Skip if same app regains focus
                    if state.current_app.as_ref() == Some(&new_app) {
                        continue;
                    }

                    // Flush previous app's session
                    if let Some(prev_app) = &state.current_app {
                        let elapsed_ms = state.focus_start.elapsed().as_millis() as u64;
                        let timestamp = current_unix_timestamp()?;
                        storage.insert_focus_event(prev_app, elapsed_ms, timestamp)?;
                    }

                    // Update current focus
                    state.current_app = Some(new_app);
                    state.focus_start = Instant::now();
                }

                Ok(_) = shutdown.changed() => {
                    // Flush current app and exit
                    if let Some(app) = &state.current_app {
                        let elapsed_ms = state.focus_start.elapsed().as_millis() as u64;
                        let timestamp = current_unix_timestamp()?;
                        storage.insert_focus_event(app, elapsed_ms, timestamp)?;
                    }
                    break;
                }

                Err(_) = &mut listener_handle => {
                    // Listener task failed; exit gracefully
                    log::warn!("Focus listener task failed; exiting");
                    if let Some(app) = &state.current_app {
                        let elapsed_ms = state.focus_start.elapsed().as_millis() as u64;
                        let timestamp = current_unix_timestamp().unwrap_or(0);
                        let _ = storage.insert_focus_event(app, elapsed_ms, timestamp);
                    }
                    break;
                }
            }
        }

        Ok(())
    }
}

/// Normalizes an app ID from the Wayland compositor.
///
/// - Strips "org.flatpak." prefix: "org.flatpak.Firefox" → "Firefox"
/// - Extracts snap app name: "snap.firefox.firefox" → "firefox" (takes the last component)
/// - Returns "Unknown" for empty/null app IDs
fn normalize_app_id(raw: Option<String>) -> String {
    raw.filter(|s| !s.is_empty())
        .map(|s| {
            if let Some(rest) = s.strip_prefix("org.flatpak.") {
                return rest.to_string();
            }
            if let Some(rest) = s.strip_prefix("snap.") {
                // Snap IDs are like "snap.firefox.firefox" — take the last component
                return rest.split('.').next_back().unwrap_or(rest).to_string();
            }
            s
        })
        .unwrap_or_else(|| "Unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flatpak_prefix_stripped() {
        assert_eq!(
            normalize_app_id(Some("org.flatpak.Firefox".to_string())),
            "Firefox"
        );
    }

    #[test]
    fn test_snap_prefix_stripped() {
        assert_eq!(
            normalize_app_id(Some("snap.firefox.firefox".to_string())),
            "firefox"
        );
    }

    #[test]
    fn test_plain_id_unchanged() {
        assert_eq!(normalize_app_id(Some("code".to_string())), "code");
    }

    #[test]
    fn test_empty_string_becomes_unknown() {
        assert_eq!(normalize_app_id(Some(String::new())), "Unknown");
    }

    #[test]
    fn test_none_becomes_unknown() {
        assert_eq!(normalize_app_id(None), "Unknown");
    }

    #[test]
    fn test_org_gnome_not_stripped() {
        assert_eq!(
            normalize_app_id(Some("org.gnome.Nautilus".to_string())),
            "org.gnome.Nautilus"
        );
    }
}
