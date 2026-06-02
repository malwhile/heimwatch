use anyhow::Result;
use heimwatch_storage::RetentionConfig;
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct DaemonConfig {
    #[serde(default)]
    pub retention: RetentionConfig,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            retention: RetentionConfig::default(),
        }
    }
}

impl DaemonConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn load_or_default(path: &Path) -> Self {
        match Self::load(path) {
            Ok(config) => config,
            Err(e) => {
                log::warn!("Failed to load config from {:?}: {}; using defaults", path, e);
                Self::default()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_config_from_toml() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"
[retention]
cpu_days = 10
net_days = 15
cleanup_interval_hours = 12
"#
        )
        .unwrap();

        let config = DaemonConfig::load(file.path()).unwrap();
        assert_eq!(config.retention.cpu_days, 10);
        assert_eq!(config.retention.net_days, 15);
        assert_eq!(config.retention.cleanup_interval_hours, 12);
        assert_eq!(config.retention.pwr_days, 7); // default
    }

    #[test]
    fn test_load_or_default_missing_file() {
        let path = Path::new("/tmp/nonexistent-heimwatch-config-12345.toml");
        let config = DaemonConfig::load_or_default(path);
        assert_eq!(config.retention.cpu_days, 7);
        assert_eq!(config.retention.cleanup_interval_hours, 24);
    }

    #[test]
    fn test_default_config() {
        let config = DaemonConfig::default();
        assert_eq!(config.retention.cpu_days, 7);
        assert_eq!(config.retention.net_days, 7);
        assert_eq!(config.retention.cleanup_interval_hours, 24);
        assert!(!config.retention.export_before_delete);
    }
}
