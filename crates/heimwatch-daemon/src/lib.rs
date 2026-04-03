//! Heimwatch daemon: polling loop that feeds network metrics into storage.

pub mod logging;

use anyhow::Result;
use heimwatch_collector::PlatformCollector;
use heimwatch_core::Collector;
use heimwatch_storage::StorageLayer;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::time::interval;

#[derive(strum_macros::Display)]
enum CollectLabel {
    Collection,
    Final,
}

impl CollectLabel {
    pub fn get_log_level(&self) -> log::Level {
        match self {
            CollectLabel::Collection => log::Level::Debug,
            CollectLabel::Final => log::Level::Info,
        }
    }
}

/// Run the heimwatch daemon with the specified poll interval and database path.
///
/// # Prerequisites
/// - Logging must be initialized before calling this function (via `logging::init_logging()`)
///
/// # Design
/// - Initializes `PlatformCollector` once at startup (wrapped in Mutex for interior mutability)
/// - Main loop uses `tokio::select!` to handle:
///   - Periodic collection via `tokio::time::interval` (triggers collection tick)
///   - OS shutdown signals (Ctrl+C)
/// - Collector runs in `tokio::task::spawn_blocking` for BPF (not Send)
/// - Storage writes also use `spawn_blocking` (sled is synchronous)
/// - Single collector instance shared across all collections via Arc<Mutex<>>
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

    // Initialize storage layer (shared across all tasks)
    let storage = Arc::new(StorageLayer::open(db_path)?);

    // Initialize collector (must be in blocking context due to BPF fds not being Send)
    // Wrap in Arc<Mutex> for shared mutable access from blocking tasks
    let collector = Arc::new(Mutex::new(PlatformCollector::new()?));
    log::debug!("PlatformCollector initialized successfully");

    // Set up periodic collection timer
    let mut collect_interval = interval(poll_interval);
    collect_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    // Main event loop with graceful shutdown
    loop {
        tokio::select! {
            // Periodic metric collection (triggered by interval)
            _ = collect_interval.tick() => {
                collect_and_persist(Arc::clone(&collector), Arc::clone(&storage), CollectLabel::Collection).await;
            }

            // Handle OS signals (Ctrl+C)
            _ = tokio::signal::ctrl_c() => {
                log::info!("Received Ctrl+C, initiating graceful shutdown...");
                break;
            }
        }
    }

    // Graceful shutdown: one final collection before exiting
    log::info!("Performing final collection before shutdown...");
    collect_and_persist(
        Arc::clone(&collector),
        Arc::clone(&storage),
        CollectLabel::Final,
    )
    .await;

    log::info!("Daemon shutdown complete");
    Ok(())
}

/// Collect network metrics and persist to storage in a blocking task.
///
/// # Arguments
/// * `collector` - Shared collector instance
/// * `storage` - Shared storage instance
/// * `label` - Context label for logging ("collection" or "final collection")
async fn collect_and_persist(
    collector: Arc<Mutex<PlatformCollector>>,
    storage: Arc<StorageLayer>,
    label: CollectLabel,
) {
    tokio::task::spawn_blocking(move || {
        let mut collector = lock_collector(&collector);
        match collector.collect_network() {
            Ok(records) => {
                if !records.is_empty() {
                    log::log!(
                        label.get_log_level(),
                        "{}: {} records",
                        label,
                        records.len()
                    );

                    if let Err(e) = storage.insert_metrics_batch(&records) {
                        log::error!("Failed to persist {}: {}", label, e);
                    }
                }
            }
            Err(e) => {
                log::warn!("Error in {}: {}", label, e);
            }
        }
    })
    .await
    .ok(); // .await.ok() is safe: all error cases handled inside spawn_blocking closure
}

fn lock_collector(collector: &Arc<Mutex<PlatformCollector>>) -> MutexGuard<'_, PlatformCollector> {
    match collector.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            log::warn!("Collector mutex poisoned; accepting risk to continue;");
            poisoned.into_inner()
        }
    }
}
