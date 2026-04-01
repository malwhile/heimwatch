//! Key encoding and decoding for sled storage.
//!
//! Keys are formatted as: `{prefix}:{timestamp:020}:{app_name}`
//!
//! The timestamp is zero-padded to 20 digits (u64::MAX width) to ensure
//! lexicographic ordering matches chronological ordering in sled's B-tree.
//! App name is variable-length and comes last.

use crate::error::StorageError;
use heimwatch_core::MetricType;

/// Encode a metric key from its components.
///
/// Format: `{prefix}:{timestamp:020}:{app_name}`
/// Example: `net:00000001711890000:firefox`
pub fn encode_key(metric_type: &MetricType, timestamp: u64, app_name: &str) -> Vec<u8> {
    let prefix = metric_type.prefix();
    let key_str = format!("{}:{:020}:{}", prefix, timestamp, app_name);
    key_str.into_bytes()
}

/// Decode a metric key back into its components.
#[allow(dead_code)]
pub fn decode_key(raw: &[u8]) -> Result<(MetricType, u64, String), StorageError> {
    let key_str = String::from_utf8(raw.to_vec())
        .map_err(|e| StorageError::InvalidKey(format!("invalid utf8: {}", e)))?;

    let parts: Vec<&str> = key_str.split(':').collect();
    if parts.len() != 3 {
        return Err(StorageError::InvalidKey(format!(
            "expected 3 parts, got {}",
            parts.len()
        )));
    }

    let prefix = parts[0];
    let metric_type = match prefix {
        "net" => MetricType::Net,
        "pwr" => MetricType::Pwr,
        "foc" => MetricType::Foc,
        "cpu" => MetricType::Cpu,
        "mem" => MetricType::Mem,
        "dsk" => MetricType::Dsk,
        "gpu" => MetricType::Gpu,
        _ => {
            return Err(StorageError::InvalidKey(format!(
                "unknown prefix: {}",
                prefix
            )));
        }
    };

    let timestamp = parts[1]
        .parse::<u64>()
        .map_err(|e| StorageError::InvalidKey(format!("invalid timestamp: {}", e)))?;

    let app_name = parts[2].to_string();

    Ok((metric_type, timestamp, app_name))
}

/// Generate the range-scan start bound for a metric type and time.
///
/// This produces the byte representation of `{prefix}:{timestamp:020}:`,
/// which, when used in a range scan, will include all records of that type
/// at or after the timestamp.
pub fn range_start(metric_type: &MetricType, timestamp: u64) -> Vec<u8> {
    let prefix = metric_type.prefix();
    let key_str = format!("{}:{:020}:", prefix, timestamp);
    key_str.into_bytes()
}

/// Generate the range-scan end bound for a metric type and time.
///
/// This produces the byte representation of `{prefix}:{timestamp:020}:~`,
/// where `~` is the highest ASCII byte, ensuring the range includes
/// all records up to and including the timestamp.
pub fn range_end(metric_type: &MetricType, timestamp: u64) -> Vec<u8> {
    let prefix = metric_type.prefix();
    let key_str = format!("{}:{:020}:~", prefix, timestamp);
    key_str.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_roundtrip() {
        let metric_type = MetricType::Cpu;
        let timestamp = 1711890000u64;
        let app_name = "firefox";

        let encoded = encode_key(&metric_type, timestamp, app_name);
        let (decoded_type, decoded_ts, decoded_app) = decode_key(&encoded).unwrap();

        assert_eq!(decoded_type, metric_type);
        assert_eq!(decoded_ts, timestamp);
        assert_eq!(decoded_app, app_name);
    }

    #[test]
    fn test_key_lexicographic_order() {
        let key1 = encode_key(&MetricType::Net, 1000, "app1");
        let key2 = encode_key(&MetricType::Net, 2000, "app2");
        let key3 = encode_key(&MetricType::Net, 10000, "app3");

        // Keys should sort in chronological order.
        assert!(key1 < key2);
        assert!(key2 < key3);
    }

    #[test]
    fn test_range_bounds() {
        let start = range_start(&MetricType::Pwr, 1000);
        let end = range_end(&MetricType::Pwr, 1000);

        // End should sort after start.
        assert!(start < end);

        // A record at exactly timestamp 1000 with an app name should fall in range.
        let record_key = encode_key(&MetricType::Pwr, 1000, "myapp");
        assert!(record_key >= start);
        assert!(record_key <= end);
    }
}
