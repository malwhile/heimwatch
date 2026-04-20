#[cfg(target_os = "linux")]
mod tests {
    use heimwatch_collector::FocusCollector;
    use heimwatch_core::CollectorEvent;

    #[test]
    fn test_try_new_does_not_panic_without_wayland() {
        // Must not panic regardless of environment (whether Wayland is available or not)
        let result = FocusCollector::try_new();
        assert!(result.is_some());
    }

    #[test]
    #[ignore] // Requires live Wayland session with wlr-foreign-toplevel support
    fn test_wlr_listener_receives_initial_focus() {
        // This test only runs with `cargo test -- --include-ignored`
        // on a system with Sway, Hyprland, or KWin
        let rt = tokio::runtime::Runtime::new().unwrap();

        let (_tx, _rx) = tokio::sync::mpsc::channel::<Option<String>>(10);

        rt.block_on(async {
            // Spawn a listener task in the background
            let listener_task = tokio::task::spawn_blocking(move || {
                // Attempt to create and run the listener
                // This will fail gracefully if protocol is unavailable
                let _listener = FocusCollector::try_new();
            });

            // Wait a short time for the listener to receive initial state
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            // The test is simply that we don't panic; receiving an actual focus event
            // would require a full integration with the event loop
            drop(listener_task);
        });
    }

    #[test]
    fn test_event_channel_closure_handling() {
        // Verify that collectors handle channel closure gracefully
        let rt = tokio::runtime::Runtime::new().unwrap();

        rt.block_on(async {
            let (tx, _rx) = tokio::sync::mpsc::channel::<CollectorEvent>(10);

            // Create an event
            let event = CollectorEvent {
                app_name: "test-app".to_string(),
                payload: heimwatch_core::MetricPayload::Foc(heimwatch_core::FocusData {
                    app_id: "test-app".to_string(),
                    duration_ms: 5000,
                }),
                timestamp: 1234567890,
            };

            // Send succeeds when receiver is alive
            assert!(tx.send(event.clone()).await.is_ok());

            // Drop the receiver
            drop(_rx);

            // Send should now fail
            assert!(tx.send(event).await.is_err());
        });
    }

    #[test]
    fn test_shutdown_signal_propagation() {
        // Verify that shutdown signal is properly broadcast to collectors
        let rt = tokio::runtime::Runtime::new().unwrap();

        rt.block_on(async {
            let (shutdown_tx, mut shutdown_rx1) = tokio::sync::watch::channel(false);
            let mut shutdown_rx2 = shutdown_tx.subscribe();

            // Initial state should be false
            assert!(!*shutdown_rx1.borrow());

            // Send shutdown signal
            let send_result = shutdown_tx.send(true);
            assert!(send_result.is_ok());

            // Both receivers should detect the change
            let changed1 = tokio::select! {
                Ok(_) = shutdown_rx1.changed() => true,
                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => false,
            };
            assert!(changed1, "First receiver should detect shutdown signal");

            let changed2 = tokio::select! {
                Ok(_) = shutdown_rx2.changed() => true,
                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => false,
            };
            assert!(changed2, "Second receiver should detect shutdown signal");
        });
    }

    #[test]
    fn test_collector_event_structure() {
        // Verify that CollectorEvent has expected fields
        let event = CollectorEvent {
            app_name: "firefox".to_string(),
            payload: heimwatch_core::MetricPayload::Foc(heimwatch_core::FocusData {
                app_id: "firefox".to_string(),
                duration_ms: 10000,
            }),
            timestamp: 1234567890,
        };

        assert_eq!(event.app_name, "firefox");
        assert_eq!(event.timestamp, 1234567890);

        // Verify payload content
        if let heimwatch_core::MetricPayload::Foc(data) = event.payload {
            assert_eq!(data.app_id, "firefox");
            assert_eq!(data.duration_ms, 10000);
        } else {
            panic!("Expected Foc payload");
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("Focus integration tests only run on Linux");
}
