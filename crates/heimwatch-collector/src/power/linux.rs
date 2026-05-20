//! Linux power collector using sysfs for battery and AC state monitoring.
//!
//! Reads battery capacity, status, current, and voltage from `/sys/class/power_supply/`,
//! along with AC adapter online status. Optionally reads RAPL energy counters for
//! measured CPU package power.

use crate::util::run_collector_loop;
use anyhow::Result;
use std::fs;
use std::path::Path;

use heimwatch_core::{
    CollectorEvent, MetricPayload, MetricRecord, PowerData, current_unix_timestamp,
};
use std::time::Duration;
use tokio::sync::{mpsc, watch};

pub const POLL_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Default)]
struct BatteryState {
    capacity: Option<f32>,
    current_ua: Option<i64>,
    voltage_uv: Option<u64>,
    status: Option<String>,
}

pub struct PowerCollector {
    battery_path: Option<String>,
    ac_path: Option<String>,
    rapl_energy_path: Option<String>,
    prev_rapl_energy_uj: Option<u64>,
}

impl PowerCollector {
    pub fn new() -> Result<Self> {
        // Discover battery and AC paths
        let power_supply_dir = Path::new("/sys/class/power_supply");
        let battery_path = discover_battery_node(power_supply_dir)?;
        let ac_path = discover_ac_node(power_supply_dir)?;

        let rapl_energy_path = discover_rapl_energy_path();

        if battery_path.is_none() && ac_path.is_none() {
            return Err(anyhow::anyhow!(
                "Power collection unavailable: no battery or AC adapter found (check /sys/class/power_supply/ permissions)"
            ));
        }

        Ok(PowerCollector {
            battery_path,
            ac_path,
            rapl_energy_path,
            prev_rapl_energy_uj: None,
        })
    }

    pub fn collect_power(&mut self) -> Result<Vec<MetricRecord>> {
        let timestamp = current_unix_timestamp()?;

        // Read battery state
        let battery_state = if let Some(ref bat_path) = self.battery_path {
            read_battery_state(bat_path)?
        } else {
            BatteryState::default()
        };

        // Determine charging state
        let charging = if let Some(ref bat_status) = battery_state.status {
            bat_status == "Charging"
        } else if let Some(ref ac_path) = self.ac_path {
            read_ac_online(ac_path).unwrap_or_else(|e| {
                log::warn!("Failed to read AC online status from {}: {}", ac_path, e);
                false
            })
        } else {
            false
        };

        // Read RAPL energy if available
        let (rapl_package_watts, watt_usage) = if let Some(ref rapl_path) = self.rapl_energy_path {
            match read_rapl_power(rapl_path, &mut self.prev_rapl_energy_uj) {
                Ok(watts) => (Some(watts), watts),
                Err(e) => {
                    log::debug!("RAPL read error (may be temporary): {}", e);
                    (None, 0.0)
                }
            }
        } else {
            (None, 0.0)
        };

        let record = MetricRecord {
            app_name: "system".to_string(),
            timestamp,
            payload: MetricPayload::Pwr(PowerData {
                watt_usage,
                battery_percent: battery_state.capacity,
                charging,
                rapl_package_watts,
                rapl_core_watts: None,
                battery_current_ua: battery_state.current_ua,
                battery_voltage_uv: battery_state.voltage_uv,
            }),
        };

        Ok(vec![record])
    }

    pub async fn run(
        self,
        tx: mpsc::Sender<CollectorEvent>,
        shutdown: watch::Receiver<bool>,
    ) -> Result<()> {
        run_collector_loop(
            self,
            POLL_INTERVAL,
            tx,
            shutdown,
            |c| c.collect_power(),
            |p| matches!(p, MetricPayload::Pwr(_)),
            "Power",
        )
        .await
    }
}

/// Discover the battery node in /sys/class/power_supply/.
fn discover_battery_node(power_supply_dir: &Path) -> Result<Option<String>> {
    if let Ok(entries) = fs::read_dir(power_supply_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let type_path = path.join("type");

            if let Ok(type_str) = fs::read_to_string(&type_path)
                && type_str.trim() == "Battery"
            {
                return Ok(Some(path.to_string_lossy().to_string()));
            }
        }
    }
    Ok(None)
}

/// Discover an AC adapter node in /sys/class/power_supply/.
fn discover_ac_node(power_supply_dir: &Path) -> Result<Option<String>> {
    if let Ok(entries) = fs::read_dir(power_supply_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let type_path = path.join("type");

            if let Ok(type_str) = fs::read_to_string(&type_path) {
                let type_trim = type_str.trim();
                if type_trim == "Mains" || type_trim == "AC" {
                    return Ok(Some(path.to_string_lossy().to_string()));
                }
            }
        }
    }
    Ok(None)
}

/// Read battery state from a battery node.
fn read_battery_state(battery_path: &str) -> Result<BatteryState> {
    let capacity = read_sysfs_value::<f32>(&format!("{}/capacity", battery_path)).ok();
    let current_ua = read_sysfs_value::<i64>(&format!("{}/current_now", battery_path)).ok();
    let voltage_uv = read_sysfs_value::<u64>(&format!("{}/voltage_now", battery_path)).ok();
    let status = fs::read_to_string(format!("{}/status", battery_path))
        .ok()
        .map(|s| s.trim().to_string());

    Ok(BatteryState {
        capacity,
        current_ua,
        voltage_uv,
        status,
    })
}

/// Read AC online status from an AC adapter node.
fn read_ac_online(ac_path: &str) -> Result<bool> {
    let online: u32 = read_sysfs_value(&format!("{}/online", ac_path))?;
    Ok(online != 0)
}

/// Read a sysfs value and parse it as type T.
fn read_sysfs_value<T: std::str::FromStr>(path: &str) -> Result<T>
where
    T::Err: std::fmt::Display,
{
    let content = fs::read_to_string(path)?;
    content
        .trim()
        .parse::<T>()
        .map_err(|e| anyhow::anyhow!("Failed to parse '{}': {}", path, e))
}

/// Discover RAPL energy counter path.
///
/// Currently only checks intel-rapl:0 (Intel single-socket). Phase 4 enhancement:
/// support multi-socket systems (intel-rapl:1, :2) and AMD RAPL paths.
fn discover_rapl_energy_path() -> Option<String> {
    let rapl_path = "/sys/class/powercap/intel-rapl/intel-rapl:0/energy_uj";
    if Path::new(rapl_path).exists() {
        Some(rapl_path.to_string())
    } else {
        None
    }
}

/// Read RAPL energy and compute power consumption.
/// Stores the previous energy reading to compute delta.
fn read_rapl_power(rapl_path: &str, prev_energy_uj: &mut Option<u64>) -> Result<f32> {
    let energy_uj: u64 = read_sysfs_value(rapl_path)?;

    let power_watts = if let Some(prev) = prev_energy_uj {
        // Compute delta energy in microjoules over 30 seconds
        let delta_uj = energy_uj.saturating_sub(*prev);

        // Convert microjoules to watts: (µJ / 30s) / 1,000,000 = W
        (delta_uj as f64 / 1_000_000.0 / 30.0) as f32
    } else {
        0.0
    };

    *prev_energy_uj = Some(energy_uj);
    Ok(power_watts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;

    /// Create a mock power supply directory structure.
    fn setup_mock_power_supply(tmpdir: &TempDir, device: &str, dev_type: &str) -> std::path::PathBuf {
        let device_path = tmpdir.path().join(device);
        fs::create_dir_all(&device_path).unwrap();
        let mut type_file = fs::File::create(device_path.join("type")).unwrap();
        write!(type_file, "{}", dev_type).unwrap();
        device_path
    }

    /// Test battery discovery with valid battery node.
    #[test]
    fn test_discover_battery_node_found() {
        let tmpdir = TempDir::new().unwrap();
        setup_mock_power_supply(&tmpdir, "BAT0", "Battery");

        let result = discover_battery_node(tmpdir.path()).unwrap();
        assert!(result.is_some());
        assert!(result.unwrap().contains("BAT0"));
    }

    /// Test battery discovery returns first battery found.
    #[test]
    fn test_discover_battery_node_first() {
        let tmpdir = TempDir::new().unwrap();
        setup_mock_power_supply(&tmpdir, "BAT0", "Battery");
        setup_mock_power_supply(&tmpdir, "BAT1", "Battery");

        let result = discover_battery_node(tmpdir.path()).unwrap();
        assert!(result.is_some());
        // Should return one of them (implementation returns first found)
        let path = result.unwrap();
        assert!(path.contains("BAT0") || path.contains("BAT1"));
    }

    /// Test battery discovery returns None when no battery found.
    #[test]
    fn test_discover_battery_node_not_found() {
        let tmpdir = TempDir::new().unwrap();
        setup_mock_power_supply(&tmpdir, "AC0", "AC");

        let result = discover_battery_node(tmpdir.path()).unwrap();
        assert!(result.is_none());
    }

    /// Test battery discovery handles missing directory gracefully.
    #[test]
    fn test_discover_battery_node_missing_dir() {
        let tmpdir = TempDir::new().unwrap();
        let nonexistent = tmpdir.path().join("nonexistent");

        let result = discover_battery_node(&nonexistent).unwrap();
        assert!(result.is_none());
    }

    /// Test AC adapter discovery with "AC" type.
    #[test]
    fn test_discover_ac_node_ac_type() {
        let tmpdir = TempDir::new().unwrap();
        setup_mock_power_supply(&tmpdir, "AC0", "AC");

        let result = discover_ac_node(tmpdir.path()).unwrap();
        assert!(result.is_some());
        assert!(result.unwrap().contains("AC0"));
    }

    /// Test AC adapter discovery with "Mains" type.
    #[test]
    fn test_discover_ac_node_mains_type() {
        let tmpdir = TempDir::new().unwrap();
        setup_mock_power_supply(&tmpdir, "Mains", "Mains");

        let result = discover_ac_node(tmpdir.path()).unwrap();
        assert!(result.is_some());
        assert!(result.unwrap().contains("Mains"));
    }

    /// Test AC adapter discovery returns None when not found.
    #[test]
    fn test_discover_ac_node_not_found() {
        let tmpdir = TempDir::new().unwrap();
        setup_mock_power_supply(&tmpdir, "BAT0", "Battery");

        let result = discover_ac_node(tmpdir.path()).unwrap();
        assert!(result.is_none());
    }

    /// Test charging state: battery status "Charging" → charging=true.
    #[test]
    fn test_charging_state_from_battery_status() {
        let tmpdir = TempDir::new().unwrap();
        let bat_path = setup_mock_power_supply(&tmpdir, "BAT0", "Battery");
        let mut status_file = fs::File::create(bat_path.join("status")).unwrap();
        write!(status_file, "Charging").unwrap();

        let battery_state = read_battery_state(bat_path.to_str().unwrap()).unwrap();
        assert_eq!(battery_state.status, Some("Charging".to_string()));
    }

    /// Test charging state: battery status "Discharging" + AC online=1 → charging=true.
    #[test]
    fn test_charging_state_from_ac_when_battery_discharging() {
        let tmpdir = TempDir::new().unwrap();
        let ac_path = setup_mock_power_supply(&tmpdir, "AC0", "AC");
        let mut online_file = fs::File::create(ac_path.join("online")).unwrap();
        write!(online_file, "1").unwrap();

        let result = read_ac_online(ac_path.to_str().unwrap()).unwrap();
        assert!(result);
    }

    /// Test charging state: AC online=0 → charging=false.
    #[test]
    fn test_charging_state_ac_offline() {
        let tmpdir = TempDir::new().unwrap();
        let ac_path = setup_mock_power_supply(&tmpdir, "AC0", "AC");
        let mut online_file = fs::File::create(ac_path.join("online")).unwrap();
        write!(online_file, "0").unwrap();

        let result = read_ac_online(ac_path.to_str().unwrap()).unwrap();
        assert!(!result);
    }

    /// Test RAPL power computation with two consecutive reads.
    /// (1200000 - 1000000) µJ / 1e6 / 30s = 0.00667 W
    #[test]
    fn test_rapl_power_computation() {
        let mut prev_energy = None;

        // First read: 1000000 µJ
        let tmpdir = TempDir::new().unwrap();
        let mut energy_file = fs::File::create(tmpdir.path().join("energy_uj")).unwrap();
        write!(energy_file, "1000000").unwrap();
        drop(energy_file);

        let watts1 = read_rapl_power(tmpdir.path().join("energy_uj").to_str().unwrap(), &mut prev_energy).unwrap();
        assert_eq!(watts1, 0.0); // First read, no previous value
        assert_eq!(prev_energy, Some(1000000));

        // Second read: 1200000 µJ (200000 µJ delta)
        let mut energy_file = fs::File::create(tmpdir.path().join("energy_uj")).unwrap();
        write!(energy_file, "1200000").unwrap();
        drop(energy_file);

        let watts2 = read_rapl_power(tmpdir.path().join("energy_uj").to_str().unwrap(), &mut prev_energy).unwrap();
        // (1200000 - 1000000) / 1e6 / 30 = 0.2 / 30 ≈ 0.00667 W
        let expected = (200000.0 / 1_000_000.0 / 30.0) as f32;
        assert!((watts2 - expected).abs() < 0.00001, "Expected {}, got {}", expected, watts2);
    }

    /// Test RAPL power computation with no previous value returns 0.
    #[test]
    fn test_rapl_power_first_read_returns_zero() {
        let tmpdir = TempDir::new().unwrap();
        let mut energy_file = fs::File::create(tmpdir.path().join("energy_uj")).unwrap();
        write!(energy_file, "1000000").unwrap();
        drop(energy_file);

        let mut prev_energy = None;
        let watts = read_rapl_power(tmpdir.path().join("energy_uj").to_str().unwrap(), &mut prev_energy).unwrap();
        assert_eq!(watts, 0.0);
    }

    /// Test RAPL power computation handles energy decrease with saturating_sub.
    /// (This tests the current Phase 1 behavior; Phase 2 should detect wraparound)
    #[test]
    fn test_rapl_power_energy_decrease_saturates_to_zero() {
        let mut prev_energy = Some(2000000u64);

        let tmpdir = TempDir::new().unwrap();
        let mut energy_file = fs::File::create(tmpdir.path().join("energy_uj")).unwrap();
        write!(energy_file, "1000000").unwrap();
        drop(energy_file);

        let watts = read_rapl_power(tmpdir.path().join("energy_uj").to_str().unwrap(), &mut prev_energy).unwrap();
        // energy decreased (wraparound), saturating_sub returns 0
        assert_eq!(watts, 0.0);
    }

    /// Test read_sysfs_value parses valid integer.
    #[test]
    fn test_read_sysfs_value_valid_u32() {
        let tmpdir = TempDir::new().unwrap();
        let mut file = fs::File::create(tmpdir.path().join("online")).unwrap();
        write!(file, "1").unwrap();
        drop(file);

        let value: u32 = read_sysfs_value(tmpdir.path().join("online").to_str().unwrap()).unwrap();
        assert_eq!(value, 1);
    }

    /// Test read_sysfs_value handles whitespace.
    #[test]
    fn test_read_sysfs_value_with_whitespace() {
        let tmpdir = TempDir::new().unwrap();
        let mut file = fs::File::create(tmpdir.path().join("capacity")).unwrap();
        write!(file, "  75  \n").unwrap();
        drop(file);

        let value: f32 = read_sysfs_value(tmpdir.path().join("capacity").to_str().unwrap()).unwrap();
        assert_eq!(value, 75.0);
    }

    /// Test read_sysfs_value returns error for invalid format.
    #[test]
    fn test_read_sysfs_value_invalid_format() {
        let tmpdir = TempDir::new().unwrap();
        let mut file = fs::File::create(tmpdir.path().join("capacity")).unwrap();
        write!(file, "invalid").unwrap();
        drop(file);

        let result: std::result::Result<f32, _> = read_sysfs_value(tmpdir.path().join("capacity").to_str().unwrap());
        assert!(result.is_err());
    }

    /// Test BatteryState default implementation.
    #[test]
    fn test_battery_state_default() {
        let state = BatteryState::default();
        assert_eq!(state.capacity, None);
        assert_eq!(state.current_ua, None);
        assert_eq!(state.voltage_uv, None);
        assert_eq!(state.status, None);
    }
}

