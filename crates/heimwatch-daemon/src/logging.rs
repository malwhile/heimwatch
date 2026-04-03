//! Centralized logging initialization for Heimwatch.
//!
//! All crates use the global `log` facade; initialize once here at startup.

use log::LevelFilter;

/// Logging configuration.
#[derive(Debug, Clone)]
pub struct LogConfig {
    /// Log level (trace, debug, info, warn, error).
    pub level: LevelFilter,
    /// Whether to use JSON format (future feature).
    pub json_format: bool,
}

impl LogConfig {
    /// Create a new log config with defaults.
    pub fn new(level: LevelFilter) -> Self {
        Self {
            level,
            json_format: false,
        }
    }

    /// Set JSON format output.
    pub fn with_json(mut self, json: bool) -> Self {
        self.json_format = json;
        self
    }
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: LevelFilter::Info,
            json_format: false,
        }
    }
}

/// Initialize the global logger for all crates in the workspace.
///
/// This should be called once at startup in `main()`.
/// All subsequent calls to `log::*` macros in any crate will use this logger.
///
/// # Example
/// ```ignore
/// let config = LogConfig::new(LevelFilter::Debug);
/// init_logging(config)?;
/// log::info!("Logger initialized"); // Works in this crate and all others
/// ```
pub fn init_logging(config: LogConfig) -> anyhow::Result<()> {
    let mut builder = env_logger::Builder::from_default_env();

    builder.filter_level(config.level).format_timestamp_millis();

    // Future: Add JSON formatting support here
    // if config.json_format {
    //     builder.format(|buf, record| {
    //         // ... JSON serialization logic
    //     });
    // }

    builder.try_init()?;
    Ok(())
}

/// Parse log level from string (e.g., "debug", "info", "warn").
pub fn parse_level(level_str: &str) -> anyhow::Result<LevelFilter> {
    match level_str.to_lowercase().as_str() {
        "off" => Ok(LevelFilter::Off),
        "error" => Ok(LevelFilter::Error),
        "warn" => Ok(LevelFilter::Warn),
        "info" => Ok(LevelFilter::Info),
        "debug" => Ok(LevelFilter::Debug),
        "trace" => Ok(LevelFilter::Trace),
        _ => Err(anyhow::anyhow!(
            "Invalid log level '{}'. Must be one of: off, error, warn, info, debug, trace",
            level_str
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_level() {
        assert_eq!(parse_level("debug").unwrap(), LevelFilter::Debug);
        assert_eq!(parse_level("INFO").unwrap(), LevelFilter::Info);
        assert_eq!(parse_level("trace").unwrap(), LevelFilter::Trace);
        assert!(parse_level("invalid").is_err());
    }

    #[test]
    fn test_log_config_default() {
        let config = LogConfig::default();
        assert_eq!(config.level, LevelFilter::Info);
        assert!(!config.json_format);
    }

    #[test]
    fn test_log_config_with_json() {
        let config = LogConfig::default().with_json(true);
        assert!(config.json_format);
    }
}
