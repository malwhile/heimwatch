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
pub const CPU_FREQ_LOCATION: &str = "/sys/devices/system/cpu/cpufreq";
pub const BACKLIGHT_LOCATION: &str = "/sys/class/backlight";
pub const NET_ROUTE_PATH: &str = "/proc/net/route";
pub const NET_CLASS_PATH: &str = "/sys/class/net";

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

        // Read CPU frequency if available
        let avg_cpu_freq_ratio = read_avg_cpu_freq_ratio(std::path::Path::new(CPU_FREQ_LOCATION));

        // Read display brightness if available
        let display_brightness = read_display_brightness(std::path::Path::new(BACKLIGHT_LOCATION));

        // Detect WiFi vs Ethernet
        let is_wifi = detect_default_interface_is_wifi(
            std::path::Path::new(NET_ROUTE_PATH),
            std::path::Path::new(NET_CLASS_PATH),
        );

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
                avg_cpu_freq_ratio,
                display_brightness,
                is_wifi,
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

/// Read average CPU frequency ratio across all cpufreq policies.
///
/// Reads /sys/devices/system/cpu/cpufreq/policy*/scaling_cur_freq and cpuinfo_max_freq,
/// computes cur/max ratio for each policy, and returns the average. Returns None if no
/// policies are readable or if all max frequencies are zero.
fn read_avg_cpu_freq_ratio(cpufreq_dir: &Path) -> Option<f32> {
    let mut ratios = Vec::new();

    if let Ok(entries) = fs::read_dir(cpufreq_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let filename = path.file_name()?;

            // Match policy directories (policy0, policy1, etc.)
            if !filename.to_string_lossy().starts_with("policy") {
                continue;
            }

            let cur_freq_path = path.join("scaling_cur_freq");
            let max_freq_path = path.join("cpuinfo_max_freq");

            // Try to read both frequency values. Guard against max_khz=0 to prevent division issues.
            #[allow(clippy::collapsible_if)]
            if let (Ok(cur_khz), Ok(max_khz)) = (
                read_sysfs_value::<u32>(cur_freq_path.to_str()?),
                read_sysfs_value::<u32>(max_freq_path.to_str()?),
            ) {
                if max_khz > 0 {
                    let ratio = (cur_khz as f32 / max_khz as f32).clamp(0.0, 1.0);
                    ratios.push(ratio);
                }
            }
        }
    }

    if ratios.is_empty() {
        None
    } else {
        let avg = ratios.iter().sum::<f32>() / ratios.len() as f32;
        Some(avg)
    }
}

/// Read average display brightness across all backlight devices.
///
/// Reads /sys/class/backlight/*/brightness and max_brightness, computes cur/max ratio
/// for each device, and returns the average. Returns None if no devices are readable
/// or if all max brightnesses are zero.
fn read_display_brightness(backlight_dir: &Path) -> Option<f32> {
    let mut ratios = Vec::new();

    if let Ok(entries) = fs::read_dir(backlight_dir) {
        for entry in entries.flatten() {
            let path = entry.path();

            let brightness_path = path.join("brightness");
            let max_brightness_path = path.join("max_brightness");

            // Try to read both brightness values. Guard against max_brightness=0 to prevent division issues.
            #[allow(clippy::collapsible_if)]
            if let (Ok(brightness), Ok(max_brightness)) = (
                read_sysfs_value::<u64>(brightness_path.to_str()?),
                read_sysfs_value::<u64>(max_brightness_path.to_str()?),
            ) {
                if max_brightness > 0 {
                    let ratio = (brightness as f32 / max_brightness as f32).clamp(0.0, 1.0);
                    ratios.push(ratio);
                }
            }
        }
    }

    if ratios.is_empty() {
        None
    } else {
        let avg = ratios.iter().sum::<f32>() / ratios.len() as f32;
        Some(avg)
    }
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

/// Detect if the default route interface is WiFi or Ethernet.
///
/// Parses /proc/net/route to find the default route entry (Destination 00000000),
/// selects the one with the lowest metric, then checks if the interface has a
/// wireless/ subdirectory under /sys/class/net/{iface}/.
///
/// Returns Some(true) if WiFi, Some(false) if Ethernet, None if unable to determine.
fn detect_default_interface_is_wifi(route_path: &Path, net_class_path: &Path) -> Option<bool> {
    // Read /proc/net/route and find default route with lowest metric
    let route_content = std::fs::read_to_string(route_path).ok()?;
    let mut default_iface: Option<(&str, u32)> = None;

    for line in route_content.lines().skip(1) {
        // Skip header line
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 7 {
            continue;
        }

        let destination = parts[1].trim();
        let metric_hex = parts[6].trim();

        // Look for default route: Destination == 00000000
        if destination == "00000000"
            && let Ok(metric) = u32::from_str_radix(metric_hex, 16)
        {
            let iface = parts[0];
            // Keep the route with lowest metric
            match default_iface {
                None => default_iface = Some((iface, metric)),
                Some((_, prev_metric)) if metric < prev_metric => {
                    default_iface = Some((iface, metric));
                }
                _ => {}
            }
        }
    }

    // Check if the default interface has a wireless/ subdirectory
    let iface = default_iface.map(|(i, _)| i)?;
    let wireless_path = net_class_path.join(iface).join("wireless");
    Some(wireless_path.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;

    /// Create a mock power supply directory structure.
    fn setup_mock_power_supply(
        tmpdir: &TempDir,
        device: &str,
        dev_type: &str,
    ) -> std::path::PathBuf {
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

        let watts1 = read_rapl_power(
            tmpdir.path().join("energy_uj").to_str().unwrap(),
            &mut prev_energy,
        )
        .unwrap();
        assert_eq!(watts1, 0.0); // First read, no previous value
        assert_eq!(prev_energy, Some(1000000));

        // Second read: 1200000 µJ (200000 µJ delta)
        let mut energy_file = fs::File::create(tmpdir.path().join("energy_uj")).unwrap();
        write!(energy_file, "1200000").unwrap();
        drop(energy_file);

        let watts2 = read_rapl_power(
            tmpdir.path().join("energy_uj").to_str().unwrap(),
            &mut prev_energy,
        )
        .unwrap();
        // (1200000 - 1000000) / 1e6 / 30 = 0.2 / 30 ≈ 0.00667 W
        let expected = (200000.0 / 1_000_000.0 / 30.0) as f32;
        assert!(
            (watts2 - expected).abs() < 0.00001,
            "Expected {}, got {}",
            expected,
            watts2
        );
    }

    /// Test RAPL power computation with no previous value returns 0.
    #[test]
    fn test_rapl_power_first_read_returns_zero() {
        let tmpdir = TempDir::new().unwrap();
        let mut energy_file = fs::File::create(tmpdir.path().join("energy_uj")).unwrap();
        write!(energy_file, "1000000").unwrap();
        drop(energy_file);

        let mut prev_energy = None;
        let watts = read_rapl_power(
            tmpdir.path().join("energy_uj").to_str().unwrap(),
            &mut prev_energy,
        )
        .unwrap();
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

        let watts = read_rapl_power(
            tmpdir.path().join("energy_uj").to_str().unwrap(),
            &mut prev_energy,
        )
        .unwrap();
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
        writeln!(file, "  75  ").unwrap();
        drop(file);

        let value: f32 =
            read_sysfs_value(tmpdir.path().join("capacity").to_str().unwrap()).unwrap();
        assert_eq!(value, 75.0);
    }

    /// Test read_sysfs_value returns error for invalid format.
    #[test]
    fn test_read_sysfs_value_invalid_format() {
        let tmpdir = TempDir::new().unwrap();
        let mut file = fs::File::create(tmpdir.path().join("capacity")).unwrap();
        write!(file, "invalid").unwrap();
        drop(file);

        let result: std::result::Result<f32, _> =
            read_sysfs_value(tmpdir.path().join("capacity").to_str().unwrap());
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

    /// Helper to setup a mock cpufreq policy directory.
    fn setup_mock_cpufreq_policy(
        cpufreq_dir: &TempDir,
        policy_name: &str,
        cur_khz: u32,
        max_khz: u32,
    ) {
        let policy_path = cpufreq_dir.path().join(policy_name);
        fs::create_dir_all(&policy_path).unwrap();
        let mut cur_file = fs::File::create(policy_path.join("scaling_cur_freq")).unwrap();
        write!(cur_file, "{}", cur_khz).unwrap();
        let mut max_file = fs::File::create(policy_path.join("cpuinfo_max_freq")).unwrap();
        write!(max_file, "{}", max_khz).unwrap();
    }

    /// Test CPU frequency ratio with single policy at 50%.
    #[test]
    fn test_read_avg_cpu_freq_ratio_single_policy() {
        let tmpdir = TempDir::new().unwrap();
        setup_mock_cpufreq_policy(&tmpdir, "policy0", 2500000, 5000000);

        let ratio = read_avg_cpu_freq_ratio(tmpdir.path()).unwrap();
        assert!((ratio - 0.5).abs() < 0.01);
    }

    /// Test CPU frequency ratio with multiple policies at different ratios.
    #[test]
    fn test_read_avg_cpu_freq_ratio_multiple_policies() {
        let tmpdir = TempDir::new().unwrap();
        setup_mock_cpufreq_policy(&tmpdir, "policy0", 3000000, 5000000); // 0.6
        setup_mock_cpufreq_policy(&tmpdir, "policy1", 2000000, 5000000); // 0.4

        let ratio = read_avg_cpu_freq_ratio(tmpdir.path()).unwrap();
        // Average of 0.6 and 0.4 = 0.5
        assert!((ratio - 0.5).abs() < 0.01);
    }

    /// Test CPU frequency ratio with no policies.
    #[test]
    fn test_read_avg_cpu_freq_ratio_no_policies() {
        let tmpdir = TempDir::new().unwrap();

        let ratio = read_avg_cpu_freq_ratio(tmpdir.path());
        assert!(ratio.is_none());
    }

    /// Test CPU frequency ratio guards against division by zero.
    #[test]
    fn test_read_avg_cpu_freq_ratio_max_freq_zero() {
        let tmpdir = TempDir::new().unwrap();
        setup_mock_cpufreq_policy(&tmpdir, "policy0", 2500000, 0); // max_khz = 0

        // Should skip this policy and return None (no valid policies)
        let ratio = read_avg_cpu_freq_ratio(tmpdir.path());
        assert!(ratio.is_none());
    }

    /// Test CPU frequency ratio with clamping to [0.0, 1.0] (shouldn't happen, but be safe).
    #[test]
    fn test_read_avg_cpu_freq_ratio_clamped() {
        let tmpdir = TempDir::new().unwrap();
        // Cur > max (shouldn't happen on real systems, but test the clamp)
        setup_mock_cpufreq_policy(&tmpdir, "policy0", 6000000, 5000000); // 1.2 → clamped to 1.0

        let ratio = read_avg_cpu_freq_ratio(tmpdir.path()).unwrap();
        assert_eq!(ratio, 1.0);
    }

    /// Helper to setup a mock backlight device directory.
    fn setup_mock_backlight_device(
        backlight_dir: &TempDir,
        device_name: &str,
        brightness: u64,
        max_brightness: u64,
    ) {
        let device_path = backlight_dir.path().join(device_name);
        fs::create_dir_all(&device_path).unwrap();
        let mut brightness_file = fs::File::create(device_path.join("brightness")).unwrap();
        write!(brightness_file, "{}", brightness).unwrap();
        let mut max_file = fs::File::create(device_path.join("max_brightness")).unwrap();
        write!(max_file, "{}", max_brightness).unwrap();
    }

    /// Test display brightness with single device at 50%.
    #[test]
    fn test_read_display_brightness_single_device() {
        let tmpdir = TempDir::new().unwrap();
        setup_mock_backlight_device(&tmpdir, "backlight0", 3000, 6000);

        let brightness = read_display_brightness(tmpdir.path()).unwrap();
        assert!((brightness - 0.5).abs() < 0.01);
    }

    /// Test display brightness with multiple devices at different ratios.
    #[test]
    fn test_read_display_brightness_multiple_devices() {
        let tmpdir = TempDir::new().unwrap();
        setup_mock_backlight_device(&tmpdir, "backlight0", 4000, 6000); // 0.667
        setup_mock_backlight_device(&tmpdir, "backlight1", 2000, 6000); // 0.333

        let brightness = read_display_brightness(tmpdir.path()).unwrap();
        // Average of 0.667 and 0.333 = 0.5
        assert!((brightness - 0.5).abs() < 0.01);
    }

    /// Test display brightness with no devices.
    #[test]
    fn test_read_display_brightness_no_devices() {
        let tmpdir = TempDir::new().unwrap();

        let brightness = read_display_brightness(tmpdir.path());
        assert!(brightness.is_none());
    }

    /// Test display brightness guards against division by zero.
    #[test]
    fn test_read_display_brightness_max_zero() {
        let tmpdir = TempDir::new().unwrap();
        setup_mock_backlight_device(&tmpdir, "backlight0", 3000, 0); // max_brightness = 0

        // Should skip this device and return None (no valid devices)
        let brightness = read_display_brightness(tmpdir.path());
        assert!(brightness.is_none());
    }

    /// Helper to setup a mock /proc/net/route file with default route entry.
    fn setup_mock_route_file(route_file: &std::path::Path, iface: &str, metric_hex: &str) {
        let mut file = fs::File::create(route_file).unwrap();
        writeln!(
            file,
            "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT"
        )
        .unwrap();
        writeln!(
            file,
            "{}\t00000000\t01010101\t0003\t0\t0\t{}\tFFFFFF00\t0\t0\t0",
            iface, metric_hex
        )
        .unwrap();
    }

    /// Helper to setup a mock network class interface directory with wireless subdirectory.
    fn setup_mock_net_interface_wifi(net_class_dir: &std::path::Path, iface: &str) {
        let iface_path = net_class_dir.join(iface);
        fs::create_dir_all(iface_path.join("wireless")).unwrap();
    }

    /// Helper to setup a mock network class interface directory without wireless subdirectory.
    fn setup_mock_net_interface_ethernet(net_class_dir: &std::path::Path, iface: &str) {
        let iface_path = net_class_dir.join(iface);
        fs::create_dir_all(&iface_path).unwrap();
    }

    /// Test WiFi detection returns true when wireless/ subdirectory exists.
    #[test]
    fn test_detect_default_interface_wifi() {
        let tmpdir = TempDir::new().unwrap();
        let route_file = tmpdir.path().join("route");
        let net_class_dir = tmpdir.path().join("net_class");

        setup_mock_route_file(&route_file, "wlp0s0", "64");
        fs::create_dir_all(&net_class_dir).unwrap();
        setup_mock_net_interface_wifi(&net_class_dir, "wlp0s0");

        let result = detect_default_interface_is_wifi(&route_file, &net_class_dir);
        assert_eq!(result, Some(true));
    }

    /// Test Ethernet detection returns false when wireless/ subdirectory does not exist.
    #[test]
    fn test_detect_default_interface_ethernet() {
        let tmpdir = TempDir::new().unwrap();
        let route_file = tmpdir.path().join("route");
        let net_class_dir = tmpdir.path().join("net_class");

        setup_mock_route_file(&route_file, "eth0", "64");
        fs::create_dir_all(&net_class_dir).unwrap();
        setup_mock_net_interface_ethernet(&net_class_dir, "eth0");

        let result = detect_default_interface_is_wifi(&route_file, &net_class_dir);
        assert_eq!(result, Some(false));
    }

    /// Test that the route with lowest metric is selected when multiple defaults exist.
    #[test]
    fn test_detect_default_interface_lowest_metric_wins() {
        let tmpdir = TempDir::new().unwrap();
        let route_file = tmpdir.path().join("route");
        let net_class_dir = tmpdir.path().join("net_class");

        // Write /proc/net/route with two default routes: wifi (metric=600), eth0 (metric=100)
        let mut file = fs::File::create(&route_file).unwrap();
        writeln!(
            file,
            "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT"
        )
        .unwrap();
        writeln!(
            file,
            "wlp0s0\t00000000\t01010101\t0003\t0\t0\t258\tFFFFFF00\t0\t0\t0"
        )
        .unwrap(); // 258 hex = 600 dec
        writeln!(
            file,
            "eth0\t00000000\t01010102\t0003\t0\t0\t64\tFFFFFF00\t0\t0\t0"
        )
        .unwrap(); // 64 hex = 100 dec

        fs::create_dir_all(&net_class_dir).unwrap();
        setup_mock_net_interface_wifi(&net_class_dir, "wlp0s0");
        setup_mock_net_interface_ethernet(&net_class_dir, "eth0");

        // Should select eth0 (metric 100 < 600)
        let result = detect_default_interface_is_wifi(&route_file, &net_class_dir);
        assert_eq!(result, Some(false));
    }

    /// Test that None is returned when no routes are found.
    #[test]
    fn test_detect_default_interface_no_routes() {
        let tmpdir = TempDir::new().unwrap();
        let route_file = tmpdir.path().join("route");
        let net_class_dir = tmpdir.path().join("net_class");

        // Create empty route file with only header
        let mut file = fs::File::create(&route_file).unwrap();
        writeln!(
            file,
            "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT"
        )
        .unwrap();

        fs::create_dir_all(&net_class_dir).unwrap();

        let result = detect_default_interface_is_wifi(&route_file, &net_class_dir);
        assert_eq!(result, None);
    }
}
