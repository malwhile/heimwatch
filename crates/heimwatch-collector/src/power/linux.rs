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
