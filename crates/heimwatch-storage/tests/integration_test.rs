mod common;

use std::sync::Arc;

use common::*;
use heimwatch_core::current_unix_timestamp;
use heimwatch_storage::{CpuData, FocusData, MetricPayload, MetricType, RetentionConfig, StorageError};

#[test]
fn test_insert_and_read_back() {
    let (db, _tmpdir) = create_test_db();

    let record = make_cpu_record("firefox", 1000, 25_500_000); // 25.5 ms in nanoseconds
    db.insert_metric(&record).unwrap();

    let results = db.get_metrics_by_type(MetricType::Cpu, 0, 2000).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].app_name, "firefox");
    assert_eq!(results[0].timestamp, 1000);

    if let MetricPayload::Cpu(CpuData { cpu_time_ns, .. }) = results[0].payload {
        assert_eq!(cpu_time_ns, 25_500_000); // 25.5 ms
    } else {
        panic!("Expected Cpu payload");
    }
}

#[test]
fn test_time_range_filtering() {
    let (db, _tmpdir) = create_test_db();

    let r1 = make_cpu_record("app", 1000, 10_000_000); // 10 ms
    let r2 = make_cpu_record("app", 2000, 20_000_000); // 20 ms
    let r3 = make_cpu_record("app", 3000, 30_000_000); // 30 ms

    db.insert_metric(&r1).unwrap();
    db.insert_metric(&r2).unwrap();
    db.insert_metric(&r3).unwrap();

    // Query range [1500, 2500] — should only return r2 (timestamp 2000)
    let results = db.get_metrics_by_type(MetricType::Cpu, 1500, 2500).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].timestamp, 2000);
}

#[test]
fn test_get_metrics_by_app() {
    let (db, _tmpdir) = create_test_db();

    let r1 = make_cpu_record("firefox", 1000, 10_000_000); // 10 ms
    let r2 = make_cpu_record("code", 1000, 20_000_000); // 20 ms
    let r3 = make_cpu_record("firefox", 2000, 15_000_000); // 15 ms

    db.insert_metric(&r1).unwrap();
    db.insert_metric(&r2).unwrap();
    db.insert_metric(&r3).unwrap();

    // Query for "firefox" only
    let results = db.get_metrics_by_app("firefox", 0, 3000).unwrap();
    assert_eq!(results.len(), 2);
    for record in results {
        assert_eq!(record.app_name, "firefox");
    }

    // Query for "code"
    let results = db.get_metrics_by_app("code", 0, 3000).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].app_name, "code");
}

#[test]
fn test_aggregated_cpu() {
    let (db, _tmpdir) = create_test_db();

    // CPU times in nanoseconds (10ms, 20ms, 30ms)
    let r1 = make_cpu_record("app", 1000, 10_000_000);
    let r2 = make_cpu_record("app", 1001, 20_000_000);
    let r3 = make_cpu_record("app", 1002, 30_000_000);

    db.insert_metric(&r1).unwrap();
    db.insert_metric(&r2).unwrap();
    db.insert_metric(&r3).unwrap();

    let mean = db.get_aggregated_cpu("app", 0, 2000).unwrap();
    assert_eq!(mean, 20_000_000.0); // Mean of 10ms, 20ms, 30ms = 20ms
}

#[test]
fn test_top_apps_by_network() {
    let (db, _tmpdir) = create_test_db();

    // firefox: 100 tx, 200 rx = 300 total
    let r1 = make_network_record("firefox", 1000, 100, 200);
    // code: 400 tx, 100 rx = 500 total
    let r2 = make_network_record("code", 1000, 400, 100);
    // slack: 50 tx, 50 rx = 100 total
    let r3 = make_network_record("slack", 1000, 50, 50);

    db.insert_metric(&r1).unwrap();
    db.insert_metric(&r2).unwrap();
    db.insert_metric(&r3).unwrap();

    let top = db.get_top_apps_by_network(0, 2000, 3).unwrap();
    assert_eq!(top.len(), 3);

    // Should be sorted: code (500), firefox (300), slack (100)
    assert_eq!(top[0].app_name, "code");
    assert_eq!(top[0].tx_bytes + top[0].rx_bytes, 500);

    assert_eq!(top[1].app_name, "firefox");
    assert_eq!(top[1].tx_bytes + top[1].rx_bytes, 300);

    assert_eq!(top[2].app_name, "slack");
    assert_eq!(top[2].tx_bytes + top[2].rx_bytes, 100);
}

#[test]
fn test_cleanup_old_data() {
    let (db, _tmpdir) = create_test_db();

    let now = current_unix_timestamp().unwrap();

    // Insert a recent record (now)
    let recent = make_cpu_record("app", now, 10_000_000); // 10 ms
    db.insert_metric(&recent).unwrap();

    // Insert an old record (8 days ago)
    let old = make_cpu_record("app", now - 8 * 86_400, 10_000_000); // 10 ms
    db.insert_metric(&old).unwrap();

    // Cleanup with 7-day retention
    let deleted = db.cleanup_old_data(7).unwrap();
    assert_eq!(deleted, 1);

    // Verify old record is gone
    let records = db
        .get_metrics_by_type(MetricType::Cpu, 0, now - 7 * 86_400)
        .unwrap();
    assert_eq!(records.len(), 0);

    // Verify recent record remains
    let records = db.get_metrics_by_type(MetricType::Cpu, 0, now).unwrap();
    assert_eq!(records.len(), 1);
}

#[test]
fn test_batch_insert() {
    let (db, _tmpdir) = create_test_db();

    let mut records = Vec::new();
    for i in 0..100 {
        // CPU times from 10ms to 109ms
        records.push(make_cpu_record(
            "app",
            1000 + i as u64,
            10_000_000 + i as u64 * 1_000_000,
        ));
    }

    db.insert_metrics_batch(&records).unwrap();

    let results = db.get_metrics_by_type(MetricType::Cpu, 0, 2000).unwrap();
    assert_eq!(results.len(), 100);
}

#[test]
fn test_metadata_operations() {
    let (db, _tmpdir) = create_test_db();

    // Default retention is 7 days
    let retention = db.get_retention_days().unwrap();
    assert_eq!(retention, 7);

    // Set to 14 days
    db.set_retention_days(14).unwrap();
    let retention = db.get_retention_days().unwrap();
    assert_eq!(retention, 14);

    // Cleanup timestamp should be None initially
    let ts = db.get_last_cleanup_ts().unwrap();
    assert_eq!(ts, None);

    // Set cleanup timestamp
    db.set_last_cleanup_ts(1000).unwrap();
    let ts = db.get_last_cleanup_ts().unwrap();
    assert_eq!(ts, Some(1000));
}

#[test]
fn test_key_collision_overwrites() {
    let (db, _tmpdir) = create_test_db();
    let r1 = make_cpu_record("app", 1000, 10_000_000); // 10 ms
    let r2 = make_cpu_record("app", 1000, 20_000_000); // 20 ms
    db.insert_metric(&r1).unwrap();
    db.insert_metric(&r2).unwrap();
    let results = db.get_metrics_by_type(MetricType::Cpu, 0, 2000).unwrap();
    assert_eq!(results.len(), 1);
    if let MetricPayload::Cpu(cpu_results) = &results[0].payload {
        assert_eq!(cpu_results.cpu_time_ns, 20_000_000);
    } else {
        panic!("Not a cpu result");
    }
}

#[test]
fn test_concurrent_inserts() {
    let (db, _tmpdir) = create_test_db();
    let db = std::sync::Arc::new(db);
    let handles: Vec<_> = (0..10)
        .map(|i| {
            let db = Arc::clone(&db);
            std::thread::spawn(move || {
                for j in 0..10 {
                    let record = make_cpu_record("app", 1000 + (i * 10 + j) as u64, 10_000_000); // 10 ms
                    db.insert_metric(&record).unwrap();
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let results = db.get_metrics_by_type(MetricType::Cpu, 0, 2000).unwrap();
    assert_eq!(results.len(), 100);
}

#[test]
fn test_aggregated_cpu_not_found() {
    let (db, _tmpdir) = create_test_db();
    let result = db.get_aggregated_cpu("nonexistent", 0, 1000);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(
        err.downcast_ref::<StorageError>(),
        Some(StorageError::NotFound)
    ));
}

#[test]
fn test_focus_event_insert_and_read_back() {
    let (db, _tmpdir) = create_test_db();

    db.insert_focus_event("firefox", 5000, 1000).unwrap();

    let results = db.get_metrics_by_type(MetricType::Foc, 0, 2000).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].app_name, "firefox");

    if let MetricPayload::Foc(FocusData {
        app_id,
        duration_ms,
    }) = &results[0].payload
    {
        assert_eq!(app_id, "firefox");
        assert_eq!(*duration_ms, 5000);
    } else {
        panic!("Expected Foc payload");
    }
}

#[test]
fn test_get_top_apps_by_focus_sorted() {
    let (db, _tmpdir) = create_test_db();

    // Insert three apps with different total focus times
    let r1 = make_focus_record("firefox", 1000, 10000); // 10s
    let r2 = make_focus_record("code", 1000, 5000); // 5s
    let r3 = make_focus_record("firefox", 2000, 15000); // 15s (total: 25s for firefox)

    db.insert_metric(&r1).unwrap();
    db.insert_metric(&r2).unwrap();
    db.insert_metric(&r3).unwrap();

    let stats = db.get_top_apps_by_focus(0, 3000, 10).unwrap();
    assert_eq!(stats.len(), 2);
    // Firefox should be first (25s total)
    assert_eq!(stats[0].app_name, "firefox");
    assert_eq!(stats[0].total_duration_ms, 25000);
    // Code should be second (5s total)
    assert_eq!(stats[1].app_name, "code");
    assert_eq!(stats[1].total_duration_ms, 5000);
}

#[test]
fn test_get_top_apps_by_focus_time_range() {
    let (db, _tmpdir) = create_test_db();

    // Insert records outside and inside a range
    let r1 = make_focus_record("app1", 500, 1000);
    let r2 = make_focus_record("app1", 1500, 2000); // Inside range
    let r3 = make_focus_record("app1", 2500, 3000);

    db.insert_metric(&r1).unwrap();
    db.insert_metric(&r2).unwrap();
    db.insert_metric(&r3).unwrap();

    // Query range [1000, 2000] — should only include r2
    let stats = db.get_top_apps_by_focus(1000, 2000, 10).unwrap();
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].total_duration_ms, 2000);
}

#[test]
fn test_get_top_apps_by_focus_limit() {
    let (db, _tmpdir) = create_test_db();

    // Insert records for 5 apps
    for i in 0..5 {
        let record = make_focus_record(&format!("app{}", i), 1000, 1000 * (i as u64 + 1));
        db.insert_metric(&record).unwrap();
    }

    // Request top 3
    let stats = db.get_top_apps_by_focus(0, 2000, 3).unwrap();
    assert_eq!(stats.len(), 3);
    // Should be sorted descending: app4 (5000ms), app3 (4000ms), app2 (3000ms)
    assert_eq!(stats[0].app_name, "app4");
    assert_eq!(stats[0].total_duration_ms, 5000);
    assert_eq!(stats[1].app_name, "app3");
    assert_eq!(stats[1].total_duration_ms, 4000);
    assert_eq!(stats[2].app_name, "app2");
    assert_eq!(stats[2].total_duration_ms, 3000);
}

#[test]
fn test_cleanup_with_config_per_type() {
    let (db, _tmpdir) = create_test_db();
    let now = current_unix_timestamp().unwrap();
    let eight_days_ago = now - (8 * 86_400);
    let three_days_ago = now - (3 * 86_400);

    // Insert old CPU record (8 days old) and new network record (3 days old)
    let old_cpu = make_cpu_record("app", eight_days_ago, 10_000_000);
    let new_net = make_network_record("app", three_days_ago, 100, 200);

    db.insert_metric(&old_cpu).unwrap();
    db.insert_metric(&new_net).unwrap();

    // Verify both are in the db
    let cpu_before = db.get_metrics_by_type(MetricType::Cpu, 0, now).unwrap();
    let net_before = db.get_metrics_by_type(MetricType::Net, 0, now).unwrap();
    assert_eq!(cpu_before.len(), 1);
    assert_eq!(net_before.len(), 1);

    // Config: cpu retention 7 days, net retention 30 days
    let mut config = RetentionConfig::default();
    config.cpu_days = 7;
    config.net_days = 30;

    let report = db.cleanup_with_config(&config).unwrap();

    // Only the old CPU record should be deleted
    assert_eq!(report.deleted_count, 1);

    // Verify CPU record is gone and net record remains
    let cpu_after = db.get_metrics_by_type(MetricType::Cpu, 0, now).unwrap();
    let net_after = db.get_metrics_by_type(MetricType::Net, 0, now).unwrap();
    assert_eq!(cpu_after.len(), 0);
    assert_eq!(net_after.len(), 1);
}

#[test]
fn test_cleanup_with_config_net_longer_retention() {
    let (db, _tmpdir) = create_test_db();
    let now = current_unix_timestamp().unwrap();
    let eight_days_ago = now - (8 * 86_400);

    // Insert old records for both CPU and net (8 days old)
    let old_cpu = make_cpu_record("app", eight_days_ago, 10_000_000);
    let old_net = make_network_record("app", eight_days_ago, 100, 200);

    db.insert_metric(&old_cpu).unwrap();
    db.insert_metric(&old_net).unwrap();

    // Config: cpu retention 7 days, net retention 30 days
    let mut config = RetentionConfig::default();
    config.cpu_days = 7;
    config.net_days = 30;

    let report = db.cleanup_with_config(&config).unwrap();

    // Only the CPU record should be deleted
    assert_eq!(report.deleted_count, 1);

    let cpu_after = db.get_metrics_by_type(MetricType::Cpu, 0, now).unwrap();
    let net_after = db.get_metrics_by_type(MetricType::Net, 0, now).unwrap();
    assert_eq!(cpu_after.len(), 0);
    assert_eq!(net_after.len(), 1);
}

#[test]
fn test_cleanup_with_config_updates_last_cleanup_ts() {
    let (db, _tmpdir) = create_test_db();

    // Verify no cleanup timestamp initially
    let ts_before = db.get_last_cleanup_ts().unwrap();
    assert_eq!(ts_before, None);

    // Run cleanup with config
    let config = RetentionConfig::default();
    db.cleanup_with_config(&config).unwrap();

    // Verify timestamp is set
    let ts_after = db.get_last_cleanup_ts().unwrap();
    assert!(ts_after.is_some());
    assert!(ts_after.unwrap() > 0);
}

#[test]
fn test_get_storage_stats() {
    let (db, _tmpdir) = create_test_db();

    // Insert various records
    let r1 = make_cpu_record("app1", 1000, 10_000_000);
    let r2 = make_cpu_record("app2", 2000, 20_000_000);
    let r3 = make_network_record("app3", 3000, 100, 200);

    db.insert_metric(&r1).unwrap();
    db.insert_metric(&r2).unwrap();
    db.insert_metric(&r3).unwrap();

    let stats = db.get_storage_stats().unwrap();

    // Check totals
    assert_eq!(stats.total_records, 3);
    assert_eq!(stats.record_counts[&MetricType::Cpu], 2);
    assert_eq!(stats.record_counts[&MetricType::Net], 1);

    // DB size may vary in testing, just verify it doesn't error
    let _ = stats.db_size_bytes;
}
