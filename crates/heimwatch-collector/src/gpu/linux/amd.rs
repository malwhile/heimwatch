//! AMD GPU backend via amdgpu driver sysfs.

use anyhow::Result;
use heimwatch_core::metrics::{GpuData, GpuVendor};
use std::path::{Path, PathBuf};

use super::generic;
use super::GpuBackend;

/// AMD GPU backend — reads metrics from amdgpu sysfs.
pub struct AmdBackend {
    gpu_index: u32,
    device_path: PathBuf,
    hwmon_path: Option<PathBuf>,
    name: String,
}

impl AmdBackend {
    pub fn new(gpu_index: u32, device_path: &Path) -> Result<Self> {
        let hwmon_path = generic::find_hwmon_path(device_path);
        let name = generic::read_device_name(device_path).unwrap_or_else(|| format!("AMD GPU {}", gpu_index));
        Ok(AmdBackend {
            gpu_index,
            device_path: device_path.to_path_buf(),
            hwmon_path,
            name,
        })
    }
}

impl GpuBackend for AmdBackend {
    fn collect(&mut self) -> Result<GpuData> {
        let mut data = GpuData {
            gpu_index: self.gpu_index,
            vendor: GpuVendor::Amd,
            name: self.name.clone(),
            usage_percent: None,
            vram_used_bytes: None,
            vram_total_bytes: None,
            temperature_celsius: None,
            power_draw_watts: None,
            core_clock_mhz: None,
            memory_clock_mhz: None,
        };

        // GPU utilization
        let util_path = self.device_path.join("gpu_busy_percent");
        if let Some(busy) = generic::read_sysfs_u64(&util_path) {
            data.usage_percent = Some(busy as f32);
        }

        // VRAM
        if let Some(used) = generic::read_sysfs_u64(&self.device_path.join("mem_info_vram_used")) {
            data.vram_used_bytes = Some(used);
        }
        if let Some(total) = generic::read_sysfs_u64(&self.device_path.join("mem_info_vram_total")) {
            data.vram_total_bytes = Some(total);
        }

        // Clock frequencies (parse pp_dpm_sclk for the line marked with *)
        data.core_clock_mhz = parse_pp_dpm_clock(&self.device_path, "pp_dpm_sclk");
        data.memory_clock_mhz = parse_pp_dpm_clock(&self.device_path, "pp_dpm_mclk");

        // Temperature and power from hwmon
        if let Some(ref hwmon) = self.hwmon_path {
            data.temperature_celsius = generic::read_hwmon_temp_celsius(hwmon);
            data.power_draw_watts = generic::read_hwmon_power_watts(hwmon);
        }

        Ok(data)
    }

    fn gpu_name(&self) -> &str {
        &self.name
    }
}

/// Parse AMD pp_dpm_sclk or pp_dpm_mclk files.
/// Format: lines like "0: 300Mhz" with the current state marked with "*".
/// Example:
/// ```
/// 0: 300Mhz
/// 1: 500Mhz *
/// ```
/// Returns the MHz value of the line marked with "*".
fn parse_pp_dpm_clock(device_path: &Path, clock_file: &str) -> Option<u32> {
    let content = std::fs::read_to_string(device_path.join(clock_file)).ok()?;
    for line in content.lines() {
        if line.contains('*') {
            // Extract the MHz value from lines like "1: 500Mhz *"
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let freq_str = parts[1].trim_end_matches("Mhz");
                if let Ok(mhz) = freq_str.parse::<u32>() {
                    return Some(mhz);
                }
            }
        }
    }
    None
}
