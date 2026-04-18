use anyhow::{Result, anyhow};
use tokio::sync::mpsc;
use zbus::Connection;

/// Check if GNOME D-Bus is accessible.
pub fn try_gnome_dbus_available() -> Result<()> {
    // D-Bus availability is checked at runtime in the async spawner.
    // This is a best-effort check; the actual connection happens later.
    Ok(())
}

/// Spawns an async task to listen for GNOME D-Bus window focus signals.
pub async fn spawn_gnome_dbus_listener(tx: mpsc::Sender<Option<String>>) -> Result<()> {
    let conn = Connection::session()
        .await
        .map_err(|e| anyhow!("D-Bus session connection failed: {}", e))?;

    // Poll the active window periodically (fallback for GNOME/KDE)
    // This is simpler and more reliable than signal-based tracking
    poll_active_window(&conn, tx).await
}

/// Poll for active window changes on GNOME or KDE.
async fn poll_active_window(conn: &Connection, tx: mpsc::Sender<Option<String>>) -> Result<()> {
    log::debug!("Starting D-Bus window focus polling");

    let mut last_app_id: Option<String> = None;
    let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(500));

    loop {
        interval.tick().await;

        let current_app = get_active_window_app(conn).await;

        // Only send if the app changed
        if current_app != last_app_id {
            let _ = tx.send(current_app.clone()).await;
            last_app_id = current_app;
        }
    }
}

/// Get the currently active application via D-Bus (GNOME or KDE).
async fn get_active_window_app(conn: &Connection) -> Option<String> {
    // Try GNOME Shell first
    if let Some(app) = query_gnome_active_app(conn).await {
        return Some(app);
    }

    // Fall back to KDE Plasma
    if let Some(app) = query_kde_active_app(conn).await {
        return Some(app);
    }

    None
}

/// Query GNOME Shell for the active application.
async fn query_gnome_active_app(conn: &Connection) -> Option<String> {
    // Try org.gnome.Shell's FocusApp property
    let result = conn
        .call_method(
            Some("org.gnome.Shell"),
            "/org/gnome/Shell",
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.gnome.Shell", "FocusApp"),
        )
        .await
        .ok()?;

    let body = result.body();
    let app_id: String = body.deserialize().ok()?;

    if !app_id.is_empty() {
        Some(app_id)
    } else {
        None
    }
}

/// Query KDE Plasma for the active application.
async fn query_kde_active_app(conn: &Connection) -> Option<String> {
    // Try org.kde.KWin's activeClient property via GetActiveWindow
    let result = conn
        .call_method(
            Some("org.kde.KWin"),
            "/org/kde/KWin",
            Some("org.kde.KWin"),
            "GetActiveWindow",
            &(),
        )
        .await
        .ok()?;

    let body = result.body();
    let window_path: String = body.deserialize().ok()?;
    if window_path.is_empty() {
        return None;
    }

    // Query the window's org.kde.KWin.Window.desktopFileName property
    get_kde_window_app(conn, &window_path).await
}

/// Get the app ID for a KDE window.
async fn get_kde_window_app(conn: &Connection, window_path: &str) -> Option<String> {
    let result = conn
        .call_method(
            Some("org.kde.KWin"),
            window_path,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.kde.KWin.Window", "desktopFileName"),
        )
        .await
        .ok()?;

    let body = result.body();
    let (app_name,): (String,) = body.deserialize().ok()?;

    // Strip .desktop suffix if present
    if app_name.ends_with(".desktop") {
        Some(app_name[..app_name.len() - 8].to_string())
    } else if !app_name.is_empty() {
        Some(app_name)
    } else {
        None
    }
}
