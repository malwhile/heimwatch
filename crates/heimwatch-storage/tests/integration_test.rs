use heimwatch_storage::{
    CpuData, MetricPayload, MetricRecord, MetricType, NetworkData, StorageLayer,
};
use tempfile::TempDir;

fn create_test_db() -> (StorageLayer, TempDir) {
    let tmpdir = TempDir::new().unwrap();
    let db = StorageLayer::open(tmpdir.path().to_str().unwrap()).unwrap();
    (db, tmpdir)
}

fn make_cpu_record(app_name: &str, timestamp: u64, usage_percent: f32) -> MetricRecord {
    MetricRecord {
        app_name: app_name.to_string(),
        timestamp,
        payload: MetricPayload::Cpu(CpuData {
            usage_percent,
            core_count: 4,
        }),
    }
}

fn make_network_record(app_name: &str, timestamp: u64, tx: u64, rx: u64) -> MetricRecord {
    MetricRecord {
        app_name: app_name.to_string(),
        timestamp,
        payload: MetricPayload::Net(NetworkData {
            tx_bytes: tx,
            rx_bytes: rx,
            connections: 1,
        }),
    }
}

#[test]
fn test_insert_and_read_back() {
    let (db, _tmpdir) = create_test_db();

    let record = make_cpu_record("firefox", 1000, 25.5);
    db.insert_metric(&record).unwrap();

    let results = db.get_metrics_by_type(MetricType::Cpu, 0, 2000).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].app_name, "firefox");
    assert_eq!(results[0].timestamp, 1000);

    if let MetricPayload::Cpu(CpuData { usage_percent, .. }) = results[0].payload {
        assert_eq!(usage_percent, 25.5);
    } else {
        panic!("Expected Cpu payload");
    }
}

#[test]
fn test_time_range_filtering() {
    let (db, _tmpdir) = create_test_db();

    let r1 = make_cpu_record("app", 1000, 10.0);
    let r2 = make_cpu_record("app", 2000, 20.0);
    let r3 = make_cpu_record("app", 3000, 30.0);

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

    let r1 = make_cpu_record("firefox", 1000, 10.0);
    let r2 = make_cpu_record("code", 1000, 20.0);
    let r3 = make_cpu_record("firefox", 2000, 15.0);

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

    let r1 = make_cpu_record("app", 1000, 10.0);
    let r2 = make_cpu_record("app", 1001, 20.0);
    let r3 = make_cpu_record("app", 1002, 30.0);

    db.insert_metric(&r1).unwrap();
    db.insert_metric(&r2).unwrap();
    db.insert_metric(&r3).unwrap();

    let mean = db.get_aggregated_cpu("app", 0, 2000).unwrap();
    assert_eq!(mean, 20.0);
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

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Insert a recent record (now)
    let recent = make_cpu_record("app", now, 10.0);
    db.insert_metric(&recent).unwrap();

    // Insert an old record (8 days ago)
    let old = make_cpu_record("app", now - 8 * 86_400, 10.0);
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
        records.push(make_cpu_record("app", 1000 + i, 10.0 + i as f32));
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
