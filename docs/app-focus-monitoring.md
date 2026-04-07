# Monitoring App Focus Time

## Overview

Focus time tracking records how long each application has the keyboard/mouse focus. This is Wayland-only (modern Linux); X11 is unsupported.

## Recommendation

- **Primary:** Connect to `wlr-foreign-toplevel-management-v1` protocol via `wayland-client`.
- **Fallback (GNOME):** If the protocol is unavailable, use D-Bus to listen to `org.gnome.Shell` window focus signals (no extensions required).
- **Graceful Degradation:** If neither works, report "Focus tracking unavailable on this compositor" and continue without error.

## Implementation Plan

### 1. Dependencies

- **wayland-client:** Wayland client library for protocol communication.
- **wayland-protocols:** XML protocol definitions (includes wlr-foreign-toplevel-management-v1).
- **zbus:** D-Bus client for GNOME fallback.
- **tokio:** Async runtime for the focus-tracking thread.
- **once_cell or lazy_static:** Thread-safe state management for current focus.

### 2. Architecture: Separate Tokio Task

The focus-change listener runs in a dedicated `tokio::select!` task:

```rust
// In the collector's initialization
let (focus_tx, focus_rx) = tokio::sync::mpsc::channel(100);
let shutdown_rx = shutdown_signal.clone();

tokio::spawn(async move {
    // Spawn focus-tracking loop here
    focus_tracking_task(focus_rx, shutdown_rx).await
});

// Main collector receives focus events on focus_tx
```

The task waits on two conditions:
1. **Focus-change event** from Wayland/D-Bus → record elapsed time, update current focus
2. **Shutdown signal** → clean up and exit

### 3. The Logic: Event-Driven with In-Memory State

```rust
// Pseudo-code for focus tracking

struct FocusState {
    current_app: Option<String>,    // app_id (normalized)
    focus_start: Instant,           // when the app gained focus
}

// On initialization
let mut focus_state = FocusState {
    current_app: None,
    focus_start: Instant::now(),
};

// When focus-change event fires
loop {
    tokio::select! {
        Some(new_app_id) = focus_event_listener.next() => {
            // Calculate elapsed time for the previous app
            if let Some(prev_app) = &focus_state.current_app {
                let elapsed = focus_state.focus_start.elapsed();
                db.insert_focus_event(prev_app, elapsed);  // Store to sled
            }

            // Update current focus
            focus_state.current_app = normalize_app_id(new_app_id);
            focus_state.focus_start = Instant::now();
        }
        
        _ = shutdown_signal.recv() => {
            // On shutdown, record final session
            if let Some(app) = &focus_state.current_app {
                let elapsed = focus_state.focus_start.elapsed();
                db.insert_focus_event(app, elapsed);
            }
            break;
        }
    }
}
```

### 4. Wayland Protocol Implementation

```rust
use wayland_client::{Connection, EventQueue};
use wayland_protocols_wlr::foreign_toplevel::v1::client::*;

// Connect and request foreign toplevel manager
let conn = Connection::connect_to_env()?;
let registry = conn.get_registry(&qh)?;

// Listen for zwlr_foreign_toplevel_manager_v1 in registry events
// On discovery, call registry.bind() to get the manager
let manager = registry.bind::<ZwlrForeignToplevelManagerV1, _, _>(...);

// For each toplevel, listen to:
// - toplevel.state() → contains focused/unfocused state
// - toplevel.app_id() → the canonical app identifier
// - toplevel.title() → human-readable window title (optional)
```

### 5. GNOME D-Bus Fallback (No Extensions)

If `wlr-foreign-toplevel-management-v1` is unavailable, fall back to D-Bus:

```rust
// Using zbus
let connection = zbus::Connection::session().await?;

// Listen to org.gnome.Shell.WindowsMonitor (or similar window manager signal)
// Some GNOME versions emit focus changes via org.gnome.Shell properties
// Alternative: use org.freedesktop.DBus.Properties.PropertiesChanged on the window object

// Parse the focused window and extract app_id from its properties
```

**Note:** GNOME's D-Bus interface is less stable than Wayland; expect some variance between GNOME versions. This fallback is best-effort.

### 6. App ID Normalization

Raw app IDs may include prefixes or be empty. Normalize them:

```rust
fn normalize_app_id(raw_id: Option<String>) -> Option<String> {
    raw_id
        .filter(|s| !s.is_empty())
        .map(|s| {
            // Strip flatpak/snap prefixes
            if s.starts_with("org.flatpak.") {
                s.strip_prefix("org.flatpak.").unwrap_or(&s).to_string()
            } else if s.starts_with("snap.") {
                s.strip_prefix("snap.").unwrap_or(&s).to_string()
            } else {
                s
            }
        })
        .or_else(|| Some("Unknown".to_string()))
}
```

- **Flatpak**: `org.flatpak.Firefox` → `Firefox`
- **Snap**: `snap.firefox.firefox` → `firefox`
- **Empty/null**: Use placeholder `"Unknown"`
- **Future**: Consider normalizing case and full paths to a canonical form (research required).

### 7. Compositor Compatibility

| Compositor | Protocol | Support | Notes |
| ---------- | -------- | ------- | ----- |
| Sway | ✅ | Excellent | Native support for wlr-foreign-toplevel. |
| Hyprland | ✅ | Excellent | Native support. |
| KDE (KWin) | ✅ | Good | Supports the protocol; may need enabling in settings. |
| GNOME (Mutter) | ⚠️ | Limited | Falls back to D-Bus; no extensions required. |
| Weston | ✅ | Good | Reference implementation, full support. |
| X11 | ❌ | Unsupported | X11 is deprecated; modern Heimwatch targets Wayland only. |

## Data Model

### Raw Focus Events

Each focus-change is stored as an event with elapsed time:

```
Schema: focus_event:[app_id]:[start_time]
Value: { app_id, duration_ms, timestamp }

Example:
  focus_event:firefox:2026-04-07T14:30:00.000Z
  → { "app_id": "firefox", "duration_ms": 187500, "timestamp": "2026-04-07T14:35:11.500Z" }
```

### Aggregation Strategy

For dashboard queries, aggregate into time buckets (5-minute windows):

```
Schema: focus_bucket:[app_id]:[bucket_start]
Value: { total_duration_ms }

Example (5-min bucket):
  focus_bucket:firefox:2026-04-07T14:30:00Z
  → { "total_duration_ms": 187500 }  // Total time in this window
```

**Timing**:
- Write raw events immediately (on focus-change).
- Aggregate lazily on read (when dashboard queries "last hour") or batch periodically (e.g., every 10 minutes for fast queries).

### Edge Cases

- **Rapid focus switching**: Each switch generates an event (correct); buckets smooth it for UI.
- **Screensaver/lock**: Do NOT count locked time; only count active focus intervals.
- **App restart**: Treat as a new focus session (separate events).
- **System UI (notifications, menus)**: May briefly steal focus; apps should track them as separate IDs (e.g., "systemd-ui").

## Privacy Considerations

- **Data collected**: Only `app_id` and `duration`. No screen content, keystrokes, or window titles (titles are optional, discarded by default).
- **Permission model**: Wayland does not require a popup if the daemon is a "trusted client"; D-Bus falls back to user-session level (same privilege as the user).
- **Local storage**: All focus events remain in the local sled database; no external transmission.
- **User transparency**: Document in README: "Focus tracking monitors which applications have keyboard/mouse focus. Data is stored locally and never transmitted."

## Testing & Troubleshooting

### Debugging

1. **Verify Wayland is running:**
   ```bash
   echo $WAYLAND_DISPLAY
   # Should output something like: wayland-0
   ```

2. **Check if the protocol is available:**
   ```bash
   wayland-info | grep -i "wlr_foreign_toplevel"
   # If found, the protocol is supported
   ```

3. **Enable detailed logging:**
   ```bash
   RUST_LOG=heimwatch_collector=debug heimwatch daemon
   # Should emit "Focus event: app_id=firefox, duration=..."
   ```

4. **Fallback detection:**
   If wayland-client fails, check daemon logs for "Falling back to D-Bus" message.
   On GNOME, verify D-Bus is accessible:
   ```bash
   busctl list | grep org.gnome.Shell
   ```

### Known Issues

- **GNOME fractional scaling**: Focus events may have slight latency; expected behavior.
- **Multiple monitors**: All focus is tracked globally (not per-monitor).
- **Wayland protocol variance**: Some compositors may emit events with missing fields (app_id, title). The daemon gracefully handles nulls (uses "Unknown").
- **D-Bus on GNOME**: Less precise than native protocol; may miss very brief focus events (<100ms).

### User-Reported Issues Checklist

- Is the user on a Wayland compositor? (Check `$WAYLAND_DISPLAY`)
- Does `wayland-info` show the protocol? If not, user is on an unsupported compositor.
- Are daemon logs showing "Focus tracking unavailable"? If so, neither protocol nor D-Bus is accessible.
- Is focus tracking recording but with gaps? Likely a brief screensaver/lock during the gap (expected).

## Final Verdict

Wayland-only, event-driven architecture with in-memory state and on-change persistence. Graceful D-Bus fallback for GNOME without requiring extensions. This approach is simple, accurate, and privacy-respecting.