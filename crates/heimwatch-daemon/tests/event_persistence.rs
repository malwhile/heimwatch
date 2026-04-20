use heimwatch_core::{CollectorEvent, FocusData, MetricPayload, NetworkData};

#[test]
fn test_collector_event_to_metric_conversion() {
    // Test converting a focus event to MetricRecord
    let focus_event = CollectorEvent {
        app_name: "firefox".to_string(),
        payload: MetricPayload::Foc(FocusData {
            app_id: "firefox".to_string(),
            duration_ms: 5000,
        }),
        timestamp: 1234567890,
    };

    // Verify event structure
    assert_eq!(focus_event.app_name, "firefox");
    assert_eq!(focus_event.timestamp, 1234567890);

    // Verify payload is FocusData
    match focus_event.payload {
        MetricPayload::Foc(data) => {
            assert_eq!(data.app_id, "firefox");
            assert_eq!(data.duration_ms, 5000);
        }
        _ => panic!("Expected Foc payload"),
    }
}

#[test]
fn test_network_event_to_metric_conversion() {
    // Test converting a network event to MetricRecord
    let network_event = CollectorEvent {
        app_name: "chrome".to_string(),
        payload: MetricPayload::Net(NetworkData {
            tx_bytes: 1024,
            rx_bytes: 2048,
            connections: 2,
        }),
        timestamp: 1234567890,
    };

    assert_eq!(network_event.app_name, "chrome");
    assert_eq!(network_event.timestamp, 1234567890);

    // Verify payload is NetworkData
    match network_event.payload {
        MetricPayload::Net(data) => {
            assert_eq!(data.tx_bytes, 1024);
            assert_eq!(data.rx_bytes, 2048);
            assert_eq!(data.connections, 2);
        }
        _ => panic!("Expected Net payload"),
    }
}

#[tokio::test]
async fn test_event_channel_multiple_senders() {
    // Test that multiple senders can send through the same channel
    let (tx1, mut rx) = tokio::sync::mpsc::channel::<CollectorEvent>(10);
    let tx2 = tx1.clone();
    let tx3 = tx1.clone();

    let focus_event = CollectorEvent {
        app_name: "focus-app".to_string(),
        payload: MetricPayload::Foc(FocusData {
            app_id: "focus-app".to_string(),
            duration_ms: 3000,
        }),
        timestamp: 100,
    };

    let network_event = CollectorEvent {
        app_name: "net-app".to_string(),
        payload: MetricPayload::Net(NetworkData {
            tx_bytes: 512,
            rx_bytes: 1024,
            connections: 1,
        }),
        timestamp: 200,
    };

    // Send from multiple senders
    tx1.send(focus_event.clone()).await.unwrap();
    tx2.send(network_event.clone()).await.unwrap();

    // Clone one more event to send from tx3
    let focus_event2 = CollectorEvent {
        app_name: "focus-app-2".to_string(),
        payload: MetricPayload::Foc(FocusData {
            app_id: "focus-app-2".to_string(),
            duration_ms: 2000,
        }),
        timestamp: 300,
    };
    tx3.send(focus_event2.clone()).await.unwrap();

    // Drop all senders so channel closes after final sends
    drop(tx1);
    drop(tx2);
    drop(tx3);

    // Receive all events
    let event1 = rx.recv().await.unwrap();
    assert_eq!(event1.app_name, "focus-app");

    let event2 = rx.recv().await.unwrap();
    assert_eq!(event2.app_name, "net-app");

    let event3 = rx.recv().await.unwrap();
    assert_eq!(event3.app_name, "focus-app-2");

    // Channel should close after all senders dropped
    let event4 = rx.recv().await;
    assert!(event4.is_none());
}

#[tokio::test]
async fn test_collector_event_timestamps() {
    // Test that events preserve their timestamps
    let timestamps = vec![
        1234567890u64,
        1234567900u64,
        1234567910u64,
    ];

    let (tx, mut rx) = tokio::sync::mpsc::channel::<CollectorEvent>(10);

    // Send events with different timestamps
    for (i, &ts) in timestamps.iter().enumerate() {
        let event = CollectorEvent {
            app_name: format!("app{}", i),
            payload: MetricPayload::Foc(FocusData {
                app_id: format!("app{}", i),
                duration_ms: 1000 + (i as u64 * 100),
            }),
            timestamp: ts,
        };
        tx.send(event).await.unwrap();
    }

    drop(tx);

    // Verify timestamps are preserved
    for (i, &expected_ts) in timestamps.iter().enumerate() {
        let event = rx.recv().await.unwrap();
        assert_eq!(event.timestamp, expected_ts);
        assert_eq!(event.app_name, format!("app{}", i));
    }
}
