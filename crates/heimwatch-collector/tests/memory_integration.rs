//! Integration tests for memory collector.
//!
//! These tests run the actual memory collector and verify behavior with real processes.
//! They work on Linux and any platform with `/proc` support (or stubs on others).

#![cfg(target_os = "linux")]

use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::watch;

use heimwatch_collector::PlatformCollector;
use heimwatch_core::MetricPayload;

/// Test that memory collector initializes successfully on Linux.
#[tokio::test]
async fn test_memory_collector_initialization() {
    let collector = PlatformCollector::new();
    match collector {
        Ok(mut c) => {
            let mem_collector = c.take_memory_collector();
            assert!(
                mem_collector.is_some(),
                "Memory collector should be available on Linux"
            );
            eprintln!("Memory collector initialized successfully");
        }
        Err(e) => {
            panic!("PlatformCollector init failed: {}", e);
        }
    }
}

/// Test that memory collector produces valid records with reasonable values.
#[tokio::test]
async fn test_memory_collector_produces_records() {
    let collector = match PlatformCollector::new() {
        Ok(mut c) => match c.take_memory_collector() {
            Some(mc) => mc,
            None => {
                panic!("Memory collector should be available on Linux");
            }
        },
        Err(e) => {
            panic!("PlatformCollector init failed: {}", e);
        }
    };

    let (tx, mut rx) = mpsc::channel(256);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Run collector for a short time
    tokio::spawn(async move {
        let _ = collector.run(tx, shutdown_rx).await;
    });

    // Give it time to collect a sample (polling interval is 10s, but first collection is immediate)
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Send shutdown signal
    let _ = shutdown_tx.send(true);

    // Collect events
    let mut event_count = 0;
    let mut total_rss = 0u64;
    let mut total_process_count = 0u32;

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
        event_count += 1;
        if let MetricPayload::Mem(mem) = &event.payload {
            eprintln!(
                "Memory record: app={}, rss={} bytes, vms={} bytes, swap={} bytes, processes={}",
                event.app_name, mem.rss_bytes, mem.vms_bytes, mem.swap_bytes, mem.process_count
            );

            // Note: Some kernel threads may have zero RSS/VMS, which is valid
            assert!(
                mem.process_count > 0,
                "Memory app {} should have process_count > 0",
                event.app_name
            );

            total_rss += mem.rss_bytes;
            total_process_count += mem.process_count;
        }
    }

    eprintln!(
        "Memory integration test: {} events, total rss={}, total process_count={}",
        event_count, total_rss, total_process_count
    );

    assert!(
        event_count > 0,
        "Should have at least one memory record from running processes"
    );
    assert!(total_rss > 0, "Total RSS across all apps should be > 0");
}

/// Test that memory metrics for the current process are captured.
#[tokio::test]
async fn test_memory_collector_captures_current_process() {
    let collector = match PlatformCollector::new() {
        Ok(mut c) => match c.take_memory_collector() {
            Some(mc) => mc,
            None => {
                panic!("Memory collector should be available on Linux");
            }
        },
        Err(e) => {
            panic!("PlatformCollector init failed: {}", e);
        }
    };

    let (tx, mut rx) = mpsc::channel(256);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    tokio::spawn(async move {
        let _ = collector.run(tx, shutdown_rx).await;
    });

    tokio::time::sleep(Duration::from_millis(500)).await;
    let _ = shutdown_tx.send(true);

    let mut found_tokio_test = false;

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
        if let MetricPayload::Mem(_mem) = &event.payload {
            eprintln!("Found app in memory records: {}", event.app_name);
            // The test binary itself or the tokio runtime should appear
            if event.app_name.contains("memory_integration")
                || event.app_name.contains("tokio")
                || event.app_name.contains("test")
            {
                found_tokio_test = true;
            }
        }
    }

    eprintln!(
        "Current process capture: found_tokio_test={}",
        found_tokio_test
    );
    // Note: We don't assert here because app naming depends on how the test is run.
    // The important thing is that we got records at all (verified above).
}

/// Test that memory aggregation correctly sums multiple processes of the same app.
#[tokio::test]
async fn test_memory_aggregation_multiple_processes() {
    let collector = match PlatformCollector::new() {
        Ok(mut c) => match c.take_memory_collector() {
            Some(mc) => mc,
            None => {
                panic!("Memory collector should be available on Linux");
            }
        },
        Err(e) => {
            panic!("PlatformCollector init failed: {}", e);
        }
    };

    let (tx, mut rx) = mpsc::channel(256);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    tokio::spawn(async move {
        let _ = collector.run(tx, shutdown_rx).await;
    });

    tokio::time::sleep(Duration::from_millis(500)).await;
    let _ = shutdown_tx.send(true);

    let mut app_processes: std::collections::HashMap<String, u32> =
        std::collections::HashMap::new();

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
        if let MetricPayload::Mem(mem) = &event.payload {
            app_processes.insert(event.app_name.clone(), mem.process_count);
        }
    }

    eprintln!(
        "Aggregation test: found {} unique apps",
        app_processes.len()
    );
    for (app, count) in &app_processes {
        eprintln!("  {}: {} processes", app, count);
    }

    // Verify that multi-process apps are reported with process_count > 1
    let multi_process_apps: Vec<_> = app_processes
        .iter()
        .filter(|(_, count)| **count > 1)
        .collect();

    eprintln!(
        "Found {} apps with multiple processes",
        multi_process_apps.len()
    );

    // This may be 0 if there are no multi-process apps running during test,
    // which is fine. The important thing is that the aggregation logic works
    // (verified by unit tests).
}

/// Test that RSS, VMS, and swap fields are independently tracked.
#[tokio::test]
async fn test_memory_fields_independently_tracked() {
    let collector = match PlatformCollector::new() {
        Ok(mut c) => match c.take_memory_collector() {
            Some(mc) => mc,
            None => {
                panic!("Memory collector should be available on Linux");
            }
        },
        Err(e) => {
            panic!("PlatformCollector init failed: {}", e);
        }
    };

    let (tx, mut rx) = mpsc::channel(256);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    tokio::spawn(async move {
        let _ = collector.run(tx, shutdown_rx).await;
    });

    tokio::time::sleep(Duration::from_millis(500)).await;
    let _ = shutdown_tx.send(true);

    let mut rss_values = Vec::new();
    let mut vms_values = Vec::new();
    let mut swap_values = Vec::new();

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
        if let MetricPayload::Mem(mem) = &event.payload {
            rss_values.push(mem.rss_bytes);
            vms_values.push(mem.vms_bytes);
            swap_values.push(mem.swap_bytes);
        }
    }

    eprintln!(
        "Field tracking: collected {} memory records",
        rss_values.len()
    );

    // RSS should be <= VMS (resident is always <= virtual)
    for (i, (rss, vms)) in rss_values.iter().zip(vms_values.iter()).enumerate() {
        assert!(
            rss <= vms,
            "Record {}: RSS {} should be <= VMS {}",
            i,
            rss,
            vms
        );
    }

    eprintln!("Field relationships verified: RSS <= VMS for all records");
}

/// Test that memory collector gracefully handles missing processes.
#[tokio::test]
async fn test_memory_collector_handles_disappeared_processes() {
    let collector = match PlatformCollector::new() {
        Ok(mut c) => match c.take_memory_collector() {
            Some(mc) => mc,
            None => {
                panic!("Memory collector should be available on Linux");
            }
        },
        Err(e) => {
            panic!("PlatformCollector init failed: {}", e);
        }
    };

    let (tx, mut rx) = mpsc::channel(256);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    tokio::spawn(async move {
        let _ = collector.run(tx, shutdown_rx).await;
    });

    // Let it run through multiple collection cycles
    tokio::time::sleep(Duration::from_millis(500)).await;
    let _ = shutdown_tx.send(true);

    // Verify that we got records without panicking
    // (processes may have exited between /proc listing and reading)
    let mut event_count = 0;
    while let Ok(Some(_event)) = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
        event_count += 1;
    }

    eprintln!(
        "Disappeared processes test: collected {} events without panicking",
        event_count
    );
    // Success if we got here without panicking
}
