mod common;

use std::sync::Arc;

use common::*;
use heimwatch_core::current_unix_timestamp;
use heimwatch_storage::{
    CpuData, FocusData, MetricPayload, MetricRecord, MetricType, RetentionConfig, StorageError,
};

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
    let config = RetentionConfig {
        cpu_days: 7,
        net_days: 30,
        ..Default::default()
    };

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
    let config = RetentionConfig {
        cpu_days: 7,
        net_days: 30,
        ..Default::default()
    };

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

#[test]
fn test_export_before_delete_with_file_io() {
    let (db, tmpdir) = create_test_db();
    let now = current_unix_timestamp().unwrap();
    let eight_days_ago = now - (8 * 86_400);

    // Insert old records that should be exported and deleted
    let old_cpu = make_cpu_record("firefox", eight_days_ago, 10_000_000);
    let old_net = make_network_record("chrome", eight_days_ago, 500, 1000);
    db.insert_metric(&old_cpu).unwrap();
    db.insert_metric(&old_net).unwrap();

    // Insert recent record that should NOT be deleted
    let recent = make_cpu_record("vscode", now, 5_000_000);
    db.insert_metric(&recent).unwrap();

    // Configure export before delete
    let export_dir = tmpdir.path().join("exports");
    let config = RetentionConfig {
        cpu_days: 7,
        net_days: 7,
        export_before_delete: true,
        export_dir: Some(export_dir.to_string_lossy().to_string()),
        ..Default::default()
    };

    // Run cleanup
    let report = db.cleanup_with_config(&config).unwrap();

    // Verify export occurred
    assert_eq!(report.deleted_count, 2, "Should delete 2 old records");
    assert_eq!(
        report.exported_count, 2,
        "Should export 2 records before deletion"
    );
    assert!(report.export_path.is_some(), "Should have export path");

    // Verify export file exists and contains valid JSONL
    let export_path = report.export_path.unwrap();
    assert!(
        export_path.exists(),
        "Export file should exist at {:?}",
        export_path
    );

    // Verify file has correct content (2 lines of JSON)
    let content = std::fs::read_to_string(&export_path).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 2, "Export file should have 2 records");

    // Verify each line is valid JSON and has expected structure
    for line in lines {
        let parsed: serde_json::Value =
            serde_json::from_str(line).expect("Line should be valid JSON");
        assert!(
            parsed.get("app_name").is_some(),
            "Record should have app_name"
        );
        assert!(
            parsed.get("timestamp").is_some(),
            "Record should have timestamp"
        );
        assert!(
            parsed.get("payload").is_some(),
            "Record should have payload"
        );
    }

    // Verify old records are deleted and recent record remains
    let cpu_after = db.get_metrics_by_type(MetricType::Cpu, 0, now).unwrap();
    let net_after = db.get_metrics_by_type(MetricType::Net, 0, now).unwrap();
    assert_eq!(cpu_after.len(), 1, "Should have 1 CPU record (recent)");
    assert_eq!(
        cpu_after[0].app_name, "vscode",
        "Remaining CPU record should be from vscode"
    );
    assert_eq!(net_after.len(), 0, "All network records should be deleted");
}

#[test]
fn test_export_filename_uniqueness_multiple_cleanups() {
    let (db, tmpdir) = create_test_db();
    let now = current_unix_timestamp().unwrap();
    let eight_days_ago = now - (8 * 86_400);

    let export_dir = tmpdir.path().join("exports");
    let config = RetentionConfig {
        cpu_days: 7,
        export_before_delete: true,
        export_dir: Some(export_dir.to_string_lossy().to_string()),
        ..Default::default()
    };

    // Run cleanup multiple times
    for i in 0..3 {
        // Insert a record each time
        let record = make_cpu_record(&format!("app{}", i), eight_days_ago, 10_000_000);
        db.insert_metric(&record).unwrap();

        let report = db.cleanup_with_config(&config).unwrap();

        // Verify export file exists
        assert!(
            report.export_path.is_some(),
            "Cleanup {} should have export path",
            i
        );
        let path = report.export_path.unwrap();
        assert!(path.exists(), "Export file {} should exist", i);
    }

    // Verify all three export files exist (no collisions)
    let export_files: Vec<_> = std::fs::read_dir(&export_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "jsonl"))
        .collect();

    assert_eq!(
        export_files.len(),
        3,
        "Should have 3 unique export files (no collisions)"
    );

    // Verify filenames have nanosecond precision (very long numbers)
    for entry in export_files {
        let filename = entry.file_name();
        let filename_str = filename.to_string_lossy();
        // Format is "export-{nanos}.jsonl"
        assert!(
            filename_str.starts_with("export-"),
            "Filename should start with 'export-'"
        );
        assert!(
            filename_str.ends_with(".jsonl"),
            "Filename should end with '.jsonl'"
        );

        // Extract the number part and verify it's a large nanosecond value
        let parts: Vec<&str> = filename_str.split('-').collect();
        assert_eq!(parts.len(), 2, "Filename should have one dash");
        let nanos_part = parts[1].trim_end_matches(".jsonl");
        let nanos: u128 = nanos_part
            .parse()
            .expect("Should be able to parse nanos as number");
        // Nanosecond timestamps are very large (19 digits typically)
        assert!(
            nanos > 1_000_000_000_000_000_000,
            "Should be nanosecond precision"
        );
    }
}

#[test]
fn test_cleanup_with_nonexistent_export_dir() {
    let (db, tmpdir) = create_test_db();
    let now = current_unix_timestamp().unwrap();
    let eight_days_ago = now - (8 * 86_400);

    let old_record = make_cpu_record("app", eight_days_ago, 10_000_000);
    db.insert_metric(&old_record).unwrap();

    // Configure export to a directory that doesn't exist (but parent does)
    let export_dir = tmpdir.path().join("nonexistent/exports");
    let config = RetentionConfig {
        export_before_delete: true,
        export_dir: Some(export_dir.to_string_lossy().to_string()),
        ..Default::default()
    };

    // Cleanup should succeed and create the directory
    let report = db.cleanup_with_config(&config).unwrap();

    assert_eq!(report.deleted_count, 1);
    assert_eq!(report.exported_count, 1);
    assert!(report.export_path.is_some());

    // Directory should have been created
    assert!(export_dir.exists(), "Export directory should be created");
    assert!(
        report.export_path.unwrap().exists(),
        "Export file should exist"
    );
}

#[test]
fn test_large_cleanup_operation() {
    let (db, _tmpdir) = create_test_db();
    let now = current_unix_timestamp().unwrap();
    let eight_days_ago = now - (8 * 86_400);

    // Insert 1000 records across all metric types that are old and will be deleted
    let mut records = Vec::new();
    for i in 0..1000 {
        let metric_type =
            heimwatch_core::ALL_METRIC_TYPES[i % heimwatch_core::ALL_METRIC_TYPES.len()];
        let record = match metric_type {
            MetricType::Cpu => make_cpu_record(&format!("app{}", i), eight_days_ago, 10_000_000),
            MetricType::Net => make_network_record(&format!("app{}", i), eight_days_ago, 100, 200),
            MetricType::Foc => make_focus_record(&format!("app{}", i), eight_days_ago, 5000),
            _ => make_cpu_record(&format!("app{}", i), eight_days_ago, 10_000_000),
        };
        records.push(record);
    }

    // Insert in batches for efficiency
    for chunk in records.chunks(100) {
        db.insert_metrics_batch(chunk).unwrap();
    }

    // Verify records are in database
    let stats_before = db.get_storage_stats().unwrap();
    assert_eq!(stats_before.total_records, 1000);

    // Run cleanup
    let config = RetentionConfig::default();
    let report = db.cleanup_with_config(&config).unwrap();

    // Verify cleanup succeeded without panicking or OOM
    assert_eq!(
        report.deleted_count, 1000,
        "Should delete all 1000 old records"
    );

    // Verify database is now empty
    let stats_after = db.get_storage_stats().unwrap();
    assert_eq!(
        stats_after.total_records, 0,
        "Database should be empty after cleanup"
    );
}

#[test]
fn test_cleanup_idempotency() {
    let (db, _tmpdir) = create_test_db();
    let now = current_unix_timestamp().unwrap();
    let eight_days_ago = now - (8 * 86_400);

    let old_cpu = make_cpu_record("app", eight_days_ago, 10_000_000);
    db.insert_metric(&old_cpu).unwrap();

    let config = RetentionConfig::default();

    // First cleanup
    let report1 = db.cleanup_with_config(&config).unwrap();
    assert_eq!(report1.deleted_count, 1);

    // Second cleanup should find nothing to delete
    let report2 = db.cleanup_with_config(&config).unwrap();
    assert_eq!(report2.deleted_count, 0);

    // Both should update the cleanup timestamp
    let ts_after = db.get_last_cleanup_ts().unwrap();
    assert!(ts_after.is_some(), "Cleanup timestamp should be set");
}

#[test]
fn test_aggregation_idempotency_tiered() {
    let (db, _tmpdir) = create_test_db();
    let now = current_unix_timestamp().unwrap();

    // Insert a record from a few days ago
    let five_days_ago = now - (5 * 86_400);
    let record = make_cpu_record("app", five_days_ago, 10_000_000);
    db.insert_metric(&record).unwrap();

    let config = heimwatch_storage::TieredRetentionConfig {
        raw_hours: 24,
        daily_keep_days: 31,
        monthly_keep_months: 12,
        yearly_keep_years: 7,
        cleanup_interval_hours: 24,
        export_before_delete: false,
        export_dir: None,
    };

    // Run tiered cleanup first time
    let report1 = db.cleanup_tiered(&config).unwrap();

    // Record should now be aggregated (raw_aggregated > 0)
    assert!(report1.raw_aggregated > 0, "Should have aggregated records");

    // Run tiered cleanup second time - should be idempotent
    let report2 = db.cleanup_tiered(&config).unwrap();

    // Both cleanups should report consistent results
    // Second run may not aggregate again (already done), but should not error
    assert!(report2.raw_deleted <= report1.raw_deleted + 1); // Allow small variance
}

#[test]
fn test_cross_tier_query_finds_both_raw_and_aggregated() {
    let (db, _tmpdir) = create_test_db();
    let now = current_unix_timestamp().unwrap();

    // Insert records at different times
    let two_days_ago = now - (2 * 86_400);
    let five_days_ago = now - (5 * 86_400);

    let recent = make_network_record("app", two_days_ago, 100, 200);
    let old = make_network_record("app", five_days_ago, 50, 75);

    db.insert_metric(&recent).unwrap();
    db.insert_metric(&old).unwrap();

    // Before cleanup: both in raw tree
    let results_before = db
        .get_metrics_by_type(MetricType::Net, five_days_ago, now)
        .unwrap();
    assert_eq!(
        results_before.len(),
        2,
        "Both records should be in raw tree initially"
    );

    let config = heimwatch_storage::TieredRetentionConfig {
        raw_hours: 48, // 2 days
        daily_keep_days: 31,
        monthly_keep_months: 12,
        yearly_keep_years: 7,
        cleanup_interval_hours: 24,
        export_before_delete: false,
        export_dir: None,
    };

    // Run cleanup: old record gets aggregated and deleted from raw, recent stays
    db.cleanup_tiered(&config).unwrap();

    // Query raw + boundary: should find recent in raw
    // (Old record is now in daily aggregate, but query stops at raw due to fallback logic)
    let results_after_raw = db
        .get_metrics_by_type(MetricType::Net, two_days_ago, now)
        .unwrap();
    assert_eq!(
        results_after_raw.len(),
        1,
        "Should find only recent record in raw tree"
    );

    // Query to force fallback to aggregates: query before raw boundary
    // Since raw will be empty for these old timestamps, fallback to daily tree
    let results_after_agg = db
        .get_metrics_by_type(MetricType::Net, five_days_ago, two_days_ago)
        .unwrap();
    assert!(
        !results_after_agg.is_empty(),
        "Should find aggregated record when querying old data"
    );
}

#[test]
fn test_query_boundary_exactly_24h_ago() {
    let (db, _tmpdir) = create_test_db();
    let now = current_unix_timestamp().unwrap();

    // Insert at various boundaries
    let exactly_24h_ago = now - (24 * 3600);
    let just_before_24h = exactly_24h_ago + 1;
    let just_after_24h = exactly_24h_ago - 1;

    let r1 = make_cpu_record("app", exactly_24h_ago, 1000);
    let r2 = make_cpu_record("app", just_before_24h, 2000);
    let r3 = make_cpu_record("app", just_after_24h, 3000);

    db.insert_metric(&r1).unwrap();
    db.insert_metric(&r2).unwrap();
    db.insert_metric(&r3).unwrap();

    // Query that includes all three
    let results = db
        .get_metrics_by_type(MetricType::Cpu, just_after_24h, now)
        .unwrap();
    assert_eq!(
        results.len(),
        3,
        "All records should be found near boundary"
    );

    // Query that excludes the oldest
    let results_after = db
        .get_metrics_by_type(MetricType::Cpu, exactly_24h_ago, now)
        .unwrap();
    assert_eq!(
        results_after.len(),
        2,
        "Should exclude record before boundary"
    );
}

#[test]
fn test_aggregation_per_metric_type_consistency() {
    let (db, _tmpdir) = create_test_db();
    let now = current_unix_timestamp().unwrap();
    let three_days_ago = now - (3 * 86_400);

    // Insert multiple different metric types for the same app
    let cpu = make_cpu_record("app", three_days_ago, 10_000_000);
    let net = make_network_record("app", three_days_ago, 100, 200);
    let mem = MetricRecord {
        app_name: "app".to_string(),
        timestamp: three_days_ago,
        payload: MetricPayload::Mem(heimwatch_core::MemoryData {
            rss_bytes: 1_000_000,
            vms_bytes: 2_000_000,
            swap_bytes: 100_000,
            process_count: 5,
        }),
    };

    db.insert_metric(&cpu).unwrap();
    db.insert_metric(&net).unwrap();
    db.insert_metric(&mem).unwrap();

    let config = heimwatch_storage::TieredRetentionConfig {
        raw_hours: 48,
        daily_keep_days: 31,
        monthly_keep_months: 12,
        yearly_keep_years: 7,
        cleanup_interval_hours: 24,
        export_before_delete: false,
        export_dir: None,
    };

    // Run cleanup
    db.cleanup_tiered(&config).unwrap();

    // Verify all metric types are still queryable
    let cpu_results = db.get_metrics_by_type(MetricType::Cpu, 0, now).unwrap();
    let net_results = db.get_metrics_by_type(MetricType::Net, 0, now).unwrap();
    let mem_results = db.get_metrics_by_type(MetricType::Mem, 0, now).unwrap();

    assert_eq!(cpu_results.len(), 1);
    assert_eq!(net_results.len(), 1);
    assert_eq!(mem_results.len(), 1);
}
