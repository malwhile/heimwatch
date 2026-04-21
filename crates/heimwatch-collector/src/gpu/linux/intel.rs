//! Intel GPU backend via i915/xe driver sysfs.

use anyhow::Result;
use heimwatch_core::metrics::{GpuData, GpuVendor};
use std::path::{Path, PathBuf};

use super::generic;
use super::GpuBackend;

/// Intel GPU backend — reads metrics from i915/xe sysfs.
/// Note: utilization requires perf PMU (Phase 5); for now, usage_percent is None.
pub struct IntelBackend {
    gpu_index: u32,
    device_path: PathBuf,
    hwmon_path: Option<PathBuf>,
    name: String,
}

impl IntelBackend {
    pub fn new(gpu_index: u32, device_path: &Path) -> Result<Self> {
        let hwmon_path = generic::find_hwmon_path(device_path);
        let name = generic::read_device_name(device_path).unwrap_or_else(|| format!("Intel GPU {}", gpu_index));
        Ok(IntelBackend {
            gpu_index,
            device_path: device_path.to_path_buf(),
            hwmon_path,
            name,
        })
    }
}

impl GpuBackend for IntelBackend {
    fn collect(&mut self) -> Result<GpuData> {
        let mut data = GpuData {
            gpu_index: self.gpu_index,
            vendor: GpuVendor::Intel,
            name: self.name.clone(),
            usage_percent: None, // Phase 5: perf PMU
            vram_used_bytes: None, // iGPU uses system RAM
            vram_total_bytes: None,
            temperature_celsius: None,
            power_draw_watts: None,
            core_clock_mhz: None,
            memory_clock_mhz: None,
        };

        // Read GPU core clock from sysfs (i915/xe: /sys/class/drm/cardN/gt/gt0/rps_cur_freq_mhz)
        let gt_path = self.device_path.join("gt/gt0");
        if let Some(freq) = generic::read_sysfs_u64(&gt_path.join("rps_cur_freq_mhz")) {
            data.core_clock_mhz = Some(freq as u32);
        }

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
