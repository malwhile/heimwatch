//! Heimwatch daemon: event-driven architecture with unified collector interface.

pub mod logging;
pub mod snapshot;

use anyhow::Result;
use heimwatch_collector::PlatformCollector;
use heimwatch_core::CollectorEvent;
use heimwatch_storage::StorageLayer;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, watch};

/// Run the heimwatch daemon with the specified poll interval and database path.
///
/// # Architecture
/// - Collectors send `CollectorEvent`s through a shared mpsc channel
/// - The daemon owns the only receiver and writes all events to the database
/// - Shutdown is broadcast via `watch::channel` to all collectors
/// - All collectors run as async tasks with event-driven coordination via tokio::select!
///
/// # Errors
/// Returns an error if:
/// - `PlatformCollector::new()` fails (e.g., missing BPF capabilities)
/// - Storage layer fails to initialize or persist records
pub async fn run(poll_interval: Duration, db_path: &str) -> Result<()> {
    log::debug!(
        "Initializing daemon loop (interval: {:?}, db: {})",
        poll_interval,
        db_path
    );

    // Initialize storage layer
    let storage = Arc::new(StorageLayer::open(db_path)?);

    // Create the unified event channel
    let (event_tx, mut event_rx) = mpsc::channel::<CollectorEvent>(256);

    // Create the shutdown broadcast
    let (shutdown_tx, _) = watch::channel(false);

    // Initialize collectors
    let mut collector = PlatformCollector::new()?;
    log::debug!("PlatformCollector initialized successfully");

    // Spawn focus collector (async task)
    if let Some(fc) = collector.take_focus_collector() {
        log::info!("Focus tracking active");
        let tx = event_tx.clone();
        let shutdown = shutdown_tx.subscribe();
        tokio::spawn(async move {
            if let Err(e) = fc.run(tx, shutdown).await {
                log::error!("Focus tracking error: {}", e);
            }
        });
    } else {
        log::debug!("Focus tracking unavailable on this platform");
    }

    // Spawn network collector (async task)
    if let Some(nc) = collector.take_network_collector() {
        log::info!("Network collection active");
        let tx = event_tx.clone();
        let shutdown = shutdown_tx.subscribe();
        tokio::spawn(async move {
            if let Err(e) = nc.run(tx, shutdown, poll_interval).await {
                log::error!("Network collection error: {}", e);
            }
        });
    } else {
        log::debug!("Network collection unavailable on this platform");
    }

    // Drop the original event_tx so the channel closes when all collectors exit
    drop(event_tx);

    // Main event loop: wait for collector events or Ctrl+C
    loop {
        tokio::select! {
            Some(event) = event_rx.recv() => {
                persist_event(&storage, event)?;
            }

            _ = tokio::signal::ctrl_c() => {
                log::info!("Received Ctrl+C, initiating graceful shutdown...");
                let _ = shutdown_tx.send(true);
                break;
            }
        }
    }

    // Drain any remaining events from collectors during shutdown
    log::info!("Draining final events before exit...");
    while let Some(event) = event_rx.recv().await {
        if let Err(e) = persist_event(&storage, event) {
            log::warn!("Error persisting final event: {}", e);
        }
    }

    log::info!("Daemon shutdown complete");
    Ok(())
}

/// Convert a `CollectorEvent` to a `MetricRecord` and persist it.
fn persist_event(storage: &Arc<StorageLayer>, event: CollectorEvent) -> Result<()> {
    use heimwatch_core::MetricRecord;

    let record = MetricRecord {
        app_name: event.app_name,
        timestamp: event.timestamp,
        payload: event.payload,
    };

    storage.insert_metric(&record)?;
    Ok(())
}
