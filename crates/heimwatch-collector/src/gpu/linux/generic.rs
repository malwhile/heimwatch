//! Generic GPU backend (hwmon only) and shared sysfs utilities.

use anyhow::Result;
use heimwatch_core::metrics::{GpuData, GpuVendor};
use std::fs;
use std::path::{Path, PathBuf};

use super::GpuBackend;

/// Read a u64 from a sysfs file.
pub fn read_sysfs_u64(path: &Path) -> Option<u64> {
    fs::read_to_string(path)
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
}

/// Read a string from a sysfs file.
pub fn read_sysfs_string(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// Find the hwmon path linked from a DRM device.
pub fn find_hwmon_path(device_path: &Path) -> Option<PathBuf> {
    let hwmon_dir = device_path.join("hwmon");
    if hwmon_dir.exists() {
        if let Ok(entries) = fs::read_dir(&hwmon_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let name = entry.file_name();
                if name.to_string_lossy().starts_with("hwmon") {
                    return Some(entry.path());
                }
            }
        }
    }
    None
}

/// Read temperature from hwmon (temp1_input in millidegrees Celsius).
pub fn read_hwmon_temp_celsius(hwmon: &Path) -> Option<f32> {
    read_sysfs_u64(&hwmon.join("temp1_input"))
        .map(|millidegrees| millidegrees as f32 / 1000.0)
}

/// Read power from hwmon (power1_average in microwatts).
pub fn read_hwmon_power_watts(hwmon: &Path) -> Option<f32> {
    read_sysfs_u64(&hwmon.join("power1_average"))
        .map(|microwatts| microwatts as f32 / 1_000_000.0)
}

/// Generic GPU backend — reads only hwmon (temperature, power).
/// Used as fallback when no vendor-specific backend is available.
pub struct GenericBackend {
    gpu_index: u32,
    hwmon_path: Option<PathBuf>,
    name: String,
}

impl GenericBackend {
    pub fn new(gpu_index: u32, device_path: &Path) -> Self {
        let hwmon_path = find_hwmon_path(device_path);
        let name = read_device_name(device_path).unwrap_or_else(|| format!("GPU {}", gpu_index));
        GenericBackend {
            gpu_index,
            hwmon_path,
            name,
        }
    }
}

impl GpuBackend for GenericBackend {
    fn collect(&mut self) -> Result<GpuData> {
        let mut data = GpuData {
            gpu_index: self.gpu_index,
            vendor: GpuVendor::Unknown,
            name: self.name.clone(),
            usage_percent: None,
            vram_used_bytes: None,
            vram_total_bytes: None,
            temperature_celsius: None,
            power_draw_watts: None,
            core_clock_mhz: None,
            memory_clock_mhz: None,
        };

        if let Some(ref hwmon) = self.hwmon_path {
            data.temperature_celsius = read_hwmon_temp_celsius(hwmon);
            data.power_draw_watts = read_hwmon_power_watts(hwmon);
        }

        Ok(data)
    }

    fn gpu_name(&self) -> &str {
        &self.name
    }
}

/// Read the device name from sysfs (product_name or PCI subsystem description).
pub fn read_device_name(device_path: &Path) -> Option<String> {
    // Try product_name first (AMD)
    if let Some(name) = read_sysfs_string(&device_path.join("product_name")) {
        return Some(name);
    }

    // Try PCI device name from sysfs
    if let Some(name) = read_sysfs_string(&device_path.join("name")) {
        return Some(name);
    }

    None
}
