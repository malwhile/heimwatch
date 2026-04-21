//! Shared utilities for collectors.

use anyhow::Result;
use heimwatch_core::{CollectorEvent, MetricPayload, MetricRecord};
use std::time::Duration;
use tokio::sync::{mpsc, watch};

/// Convert a null-terminated byte array (from kernel comm field) to a String.
/// Returns None if the comm is empty or invalid UTF-8.
pub fn comm_to_string(comm: &[u8; 16]) -> Option<String> {
    // Find the null terminator
    let end = comm.iter().position(|&b| b == 0).unwrap_or(16);

    // Empty comm field
    if end == 0 {
        return None;
    }

    // Convert to UTF-8 string, replacing invalid bytes with replacement character
    Some(String::from_utf8_lossy(&comm[..end]).into_owned())
}

/// Generic collector run loop template for eBPF-based collectors.
///
/// Handles the boilerplate: interval ticking, collection, event dispatch, shutdown signaling.
///
/// # Type Parameters
/// - `C`: Collector type (must have a collect method returning Vec<MetricRecord>)
/// - `F`: Collect function type
/// - `P`: Payload type predicate (closure that checks if a MetricPayload matches this collector's type)
///
/// # Arguments
/// - `collector`: The collector instance
/// - `poll_interval`: How often to collect metrics
/// - `tx`: Channel for sending CollectorEvents
/// - `shutdown`: Watch receiver for shutdown signal
/// - `collect_fn`: Function that collects metrics (e.g., `|c| c.collect_cpu(interval)`)
/// - `payload_check`: Predicate to filter records by payload type (e.g., `|p| matches!(p, MetricPayload::Cpu(_))`)
/// - `collector_name`: Name for logging (e.g., "CPU")
pub async fn run_collector_loop<C, F, P>(
    mut collector: C,
    poll_interval: Duration,
    tx: mpsc::Sender<CollectorEvent>,
    mut shutdown: watch::Receiver<bool>,
    collect_fn: F,
    payload_check: P,
    collector_name: &str,
) -> Result<()>
where
    F: Fn(&mut C) -> Result<Vec<MetricRecord>> + Send,
    P: Fn(&MetricPayload) -> bool + Send,
{
    let mut ticker = tokio::time::interval(poll_interval);

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                match collect_fn(&mut collector) {
                    Ok(records) => {
                        for record in records {
                            if payload_check(&record.payload) {
                                let event = CollectorEvent {
                                    app_name: record.app_name.clone(),
                                    payload: record.payload,
                                    timestamp: record.timestamp,
                                };
                                if let Err(e) = tx.send(event).await {
                                    log::error!(
                                        "Failed to send {} event for app '{}': {}",
                                        collector_name,
                                        record.app_name,
                                        e
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("{} collection error: {}", collector_name, e);
                    }
                }
            }

            _ = shutdown.changed() => {
                log::debug!("{} collector received shutdown signal", collector_name);
                break;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_comm_to_string_valid() {
        let mut comm = [0u8; 16];
        b"firefox"
            .iter()
            .enumerate()
            .for_each(|(i, &b)| comm[i] = b);
        assert_eq!(comm_to_string(&comm), Some("firefox".to_string()));
    }

    #[test]
    fn test_comm_to_string_empty() {
        let comm = [0u8; 16];
        assert_eq!(comm_to_string(&comm), None);
    }

    #[test]
    fn test_comm_to_string_truncated() {
        let mut comm = [0u8; 16];
        b"python3.11"
            .iter()
            .enumerate()
            .for_each(|(i, &b)| comm[i] = b);
        comm[6] = 0;
        assert_eq!(comm_to_string(&comm), Some("python".to_string()));
    }
}
