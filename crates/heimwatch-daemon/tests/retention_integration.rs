use heimwatch_daemon::config::DaemonConfig;
use heimwatch_storage::RetentionConfig;
use std::fs;
use tempfile::NamedTempFile;

#[test]
fn test_daemon_config_defaults() {
    let config = DaemonConfig::default();
    assert_eq!(config.retention.cpu_days, 7);
    assert_eq!(config.retention.net_days, 7);
    assert_eq!(config.retention.cleanup_interval_hours, 24);
    assert!(!config.retention.export_before_delete);
}

#[test]
fn test_daemon_config_from_toml() {
    let toml_content = r#"
[retention]
cpu_days = 10
net_days = 15
cleanup_interval_hours = 12
export_before_delete = false
"#;

    let file = NamedTempFile::new().unwrap();
    fs::write(file.path(), toml_content).unwrap();

    let config = DaemonConfig::load(file.path()).unwrap();
    assert_eq!(config.retention.cpu_days, 10);
    assert_eq!(config.retention.net_days, 15);
    assert_eq!(config.retention.cleanup_interval_hours, 12);
    assert!(!config.retention.export_before_delete);
    // Unspecified fields should use defaults
    assert_eq!(config.retention.pwr_days, 7);
    assert_eq!(config.retention.dsk_days, 7);
}

#[test]
fn test_daemon_config_with_export() {
    let toml_content = r#"
[retention]
cpu_days = 7
cleanup_interval_hours = 24
export_before_delete = true
export_dir = "./exports"
"#;

    let file = NamedTempFile::new().unwrap();
    fs::write(file.path(), toml_content).unwrap();

    let config = DaemonConfig::load(file.path()).unwrap();
    assert!(config.retention.export_before_delete);
    assert_eq!(config.retention.export_dir, Some("./exports".to_string()));
}

#[test]
fn test_retention_config_retention_days_for() {
    let mut config = RetentionConfig::default();
    config.cpu_days = 10;
    config.net_days = 20;

    assert_eq!(
        config.retention_days_for(heimwatch_core::MetricType::Cpu),
        10
    );
    assert_eq!(
        config.retention_days_for(heimwatch_core::MetricType::Net),
        20
    );
    assert_eq!(
        config.retention_days_for(heimwatch_core::MetricType::Pwr),
        7
    ); // default
}
