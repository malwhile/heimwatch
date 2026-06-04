use heimwatch_daemon::config::DaemonConfig;
use heimwatch_storage::RetentionConfig;
use std::fs;
use tempfile::NamedTempFile;

#[test]
fn test_daemon_config_defaults() {
    let config = DaemonConfig::default();
    assert_eq!(config.tiered_retention.raw_hours, 24);
    assert_eq!(config.tiered_retention.daily_keep_days, 31);
    assert_eq!(config.tiered_retention.cleanup_interval_hours, 24);
    assert!(!config.tiered_retention.export_before_delete);
}

#[test]
fn test_daemon_config_from_toml() {
    let toml_content = r#"
[retention]
raw_hours = 12
daily_keep_days = 30
cleanup_interval_hours = 12
export_before_delete = false
"#;

    let file = NamedTempFile::new().unwrap();
    fs::write(file.path(), toml_content).unwrap();

    let config = DaemonConfig::load(file.path()).unwrap();
    assert_eq!(config.tiered_retention.raw_hours, 12);
    assert_eq!(config.tiered_retention.daily_keep_days, 30);
    assert_eq!(config.tiered_retention.cleanup_interval_hours, 12);
    assert!(!config.tiered_retention.export_before_delete);
    // Unspecified fields should use defaults
    assert_eq!(config.tiered_retention.monthly_keep_months, 12);
    assert_eq!(config.tiered_retention.yearly_keep_years, 7);
}

#[test]
fn test_daemon_config_with_export() {
    let toml_content = r#"
[retention]
raw_hours = 24
cleanup_interval_hours = 24
export_before_delete = true
export_dir = "./exports"
"#;

    let file = NamedTempFile::new().unwrap();
    fs::write(file.path(), toml_content).unwrap();

    let config = DaemonConfig::load(file.path()).unwrap();
    assert!(config.tiered_retention.export_before_delete);
    assert_eq!(
        config.tiered_retention.export_dir,
        Some("./exports".to_string())
    );
}

#[test]
fn test_retention_config_retention_days_for() {
    let config = RetentionConfig {
        cpu_days: 10,
        net_days: 20,
        ..Default::default()
    };

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
