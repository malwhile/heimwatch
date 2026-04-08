use anyhow::{Result, anyhow};
use std::collections::HashMap;
use tokio::sync::mpsc;
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle,
    globals::{GlobalListContents, registry_queue_init},
    protocol::wl_registry,
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1,
    zwlr_foreign_toplevel_handle_v1::ZwlrForeignToplevelHandleV1,
    zwlr_foreign_toplevel_manager_v1,
    zwlr_foreign_toplevel_manager_v1::{EVT_TOPLEVEL_OPCODE, ZwlrForeignToplevelManagerV1},
};

use super::ToplevelInfo;

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

/// Check if the wlr-foreign-toplevel protocol is available.
pub fn try_wlr_toplevel_available() -> Result<()> {
    let conn =
        Connection::connect_to_env().map_err(|e| anyhow!("Wayland connection failed: {}", e))?;

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

/// Wayland event dispatcher state for focus tracking.
pub struct WlrToplevelState {
    toplevels: HashMap<wayland_client::backend::ObjectId, ToplevelInfo>,
    tx: mpsc::Sender<Option<String>>,
}

/// Spawns a blocking task to listen for Wayland wlr-foreign-toplevel events.
pub fn spawn_wlr_toplevel_listener(tx: mpsc::Sender<Option<String>>) -> Result<()> {
    let conn =
        Connection::connect_to_env().map_err(|e| anyhow!("Wayland connection failed: {}", e))?;

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
                    .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
                    .any(|v| v == zwlr_foreign_toplevel_handle_v1::State::Activated as u32);

                if let Some(info) = state.toplevels.get_mut(&id) {
                    info.is_activated = activated;
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::Done => {
                // Properties are committed; check if focused and send
                if let Some(info) = state.toplevels.get(&id)
                    && info.is_activated
                {
                    let _ = state.tx.try_send(info.app_id.clone());
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::Closed => {
                state.toplevels.remove(&id);
            }
            _ => {}
        }
    }
}
