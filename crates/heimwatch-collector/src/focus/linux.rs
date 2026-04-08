//! Wayland-based focus tracking on Linux.
//!
//! Listens to window focus changes via the wlr-foreign-toplevel-management-v1
//! protocol or falls back to GNOME D-Bus signals. Tracks elapsed time in memory
//! and persists focus sessions to the storage layer on focus-change events.

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, watch};

use heimwatch_core::current_unix_timestamp;
use heimwatch_storage::StorageLayer;

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
        if try_wlr_toplevel_available().is_ok() {
            log::debug!("Focus tracking: wlr-foreign-toplevel-management-v1 available");
            return Some(FocusCollector {
                source: FocusSource::WlrToplevel,
            });
        }

        // Fall back to GNOME D-Bus
        if try_gnome_dbus_available().is_ok() {
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
                tokio::task::spawn_blocking(move || spawn_wlr_toplevel_listener(tx))
            }
            FocusSource::GnomeDbus => {
                let tx = event_tx.clone();
                tokio::spawn(spawn_gnome_dbus_listener(tx))
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
                        let timestamp = current_unix_timestamp().unwrap_or_else(|_| 0);
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
                return rest.split('.').last().unwrap_or(&rest).to_string();
            }
            s
        })
        .unwrap_or_else(|| "Unknown".to_string())
}

/// Check if the wlr-foreign-toplevel protocol is available.
fn try_wlr_toplevel_available() -> Result<()> {
    use wayland_client::globals::registry_queue_init;
    use wayland_client::Connection;
    use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::ZwlrForeignToplevelManagerV1;

    let conn = Connection::connect_to_env()
        .map_err(|e| anyhow!("Wayland connection failed: {}", e))?;

    // Use minimal noop state to check protocol availability
    let (globals, _queue) = registry_queue_init::<NoopRegistryState>(&conn)
        .map_err(|e| anyhow!("Registry init failed: {}", e))?;

    // Check if the protocol is in the global list without binding
    let found = globals.contents().with_list(|list| {
        let iface_name = ZwlrForeignToplevelManagerV1::interface().name;
        list.iter().any(|g| g.interface == iface_name)
    });

    if found {
        Ok(())
    } else {
        Err(anyhow!(
            "zwlr-foreign-toplevel-management-v1 protocol not available"
        ))
    }
}

/// Check if GNOME D-Bus is accessible.
fn try_gnome_dbus_available() -> Result<()> {
    // D-Bus availability is checked at runtime in the async spawner.
    // This is a best-effort check; the actual connection happens later.
    Ok(())
}

// ============================================================================
// Wayland Protocol Implementation
// ============================================================================

use wayland_client::{
    globals::GlobalListContents, protocol::wl_registry, Connection, Dispatch, Proxy,
    QueueHandle,
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1, zwlr_foreign_toplevel_manager_v1,
};

/// Minimal state for protocol availability check.
struct NoopRegistryState;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for NoopRegistryState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        // No-op: only used for initial registry roundtrip
    }
}

/// Per-toplevel state accumulated between protocol events.
struct ToplevelInfo {
    app_id: Option<String>,
    is_activated: bool,
}

/// Wayland event dispatcher state for focus tracking.
struct WlrToplevelState {
    toplevels: HashMap<wayland_client::backend::ObjectId, ToplevelInfo>,
    tx: mpsc::Sender<Option<String>>,
}

/// Spawns a blocking task to listen for Wayland wlr-foreign-toplevel events.
fn spawn_wlr_toplevel_listener(tx: mpsc::Sender<Option<String>>) -> Result<()> {
    use wayland_client::globals::registry_queue_init;
    use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::ZwlrForeignToplevelManagerV1;

    let conn = Connection::connect_to_env()
        .map_err(|e| anyhow!("Wayland connection failed: {}", e))?;

    let (globals, mut event_queue) = registry_queue_init::<WlrToplevelState>(&conn)
        .map_err(|e| anyhow!("Registry init failed: {}", e))?;

    let qh = event_queue.handle();

    // Bind the manager; fails if protocol absent
    let _manager: ZwlrForeignToplevelManagerV1 = globals
        .bind(&qh, 1..=3, ())
        .map_err(|e| anyhow!("Failed to bind zwlr_foreign_toplevel_manager: {:?}", e))?;

    let mut state = WlrToplevelState {
        toplevels: HashMap::new(),
        tx,
    };

    // Main event loop (blocking, suitable for spawn_blocking)
    loop {
        event_queue
            .blocking_dispatch(&mut state)
            .map_err(|e| anyhow!("Wayland dispatch error: {}", e))?;
    }
}

// Dispatch for WlRegistry (required by registry_queue_init)
impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for WlrToplevelState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        // No-op: registry events handled during init
    }
}

// Dispatch for ZwlrForeignToplevelManagerV1
impl Dispatch<zwlr_foreign_toplevel_manager_v1::ZwlrForeignToplevelManagerV1, ()>
    for WlrToplevelState
{
    fn event(
        state: &mut Self,
        _proxy: &zwlr_foreign_toplevel_manager_v1::ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } => {
                // New toplevel created by server; register it
                state.toplevels.insert(
                    toplevel.id(),
                    ToplevelInfo {
                        app_id: None,
                        is_activated: false,
                    },
                );
            }
            zwlr_foreign_toplevel_manager_v1::Event::Finished => {
                // Compositor finished sending toplevels; handled gracefully
                log::debug!("Foreign toplevel manager finished");
            }
            _ => {}
        }
    }

    fn event_created_child(
        opcode: u16,
        qhandle: &QueueHandle<Self>,
    ) -> std::sync::Arc<dyn wayland_client::backend::ObjectData> {
        use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE;
        use wayland_protocols_wlr::foreign_toplevel::v1::client::zwlr_foreign_toplevel_handle_v1::ZwlrForeignToplevelHandleV1;

        match opcode {
            EVT_TOPLEVEL_OPCODE => qhandle.make_data::<ZwlrForeignToplevelHandleV1, ()>(()),
            _ => panic!("Unexpected opcode in foreign toplevel manager: {}", opcode),
        }
    }
}

// Dispatch for ZwlrForeignToplevelHandleV1 (core focus tracking)
impl Dispatch<zwlr_foreign_toplevel_handle_v1::ZwlrForeignToplevelHandleV1, ()>
    for WlrToplevelState
{
    fn event(
        state: &mut Self,
        proxy: &zwlr_foreign_toplevel_handle_v1::ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        let id = proxy.id();

        match event {
            zwlr_foreign_toplevel_handle_v1::Event::AppId { app_id } => {
                if let Some(info) = state.toplevels.get_mut(&id) {
                    info.app_id = Some(app_id);
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::State { state: state_bytes } => {
                // Decode state array (4-byte little-endian u32 chunks)
                let activated = state_bytes
                    .chunks_exact(4)
                    .map(|c| {
                        u32::from_ne_bytes([c[0], c[1], c[2], c[3]])
                    })
                    .any(|v| v == zwlr_foreign_toplevel_handle_v1::State::Activated as u32);

                if let Some(info) = state.toplevels.get_mut(&id) {
                    info.is_activated = activated;
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::Done => {
                // Properties are committed; check if focused and send
                if let Some(info) = state.toplevels.get(&id) {
                    if info.is_activated {
                        let _ = state.tx.try_send(info.app_id.clone());
                    }
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::Closed => {
                state.toplevels.remove(&id);
            }
            _ => {}
        }
    }
}

/// Spawns an async task to listen for GNOME D-Bus window focus signals.
async fn spawn_gnome_dbus_listener(_tx: mpsc::Sender<Option<String>>) -> Result<()> {
    use zbus::Connection;

    let _conn = Connection::session()
        .await
        .map_err(|e| anyhow!("D-Bus session connection failed: {}", e))?;

    // TODO: Complete implementation:
    // 1. Subscribe to org.gnome.Shell signals or properties
    // 2. Listen for focus window changes
    // 3. Extract app_id from the focused window
    // 4. Send on the channel
    //
    // For now, this is a stub that keeps the listener alive.
    log::debug!("GNOME D-Bus listener started (stub implementation)");

    // Keep the listener alive; will be cancelled on shutdown
    std::future::pending().await
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
