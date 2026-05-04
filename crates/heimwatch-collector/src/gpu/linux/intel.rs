//! Intel GPU backend via i915/xe driver sysfs.

use anyhow::Result;
use heimwatch_core::metrics::{GpuData, GpuVendor};
use std::path::{Path, PathBuf};

use super::GpuBackend;
use super::generic;

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
        let name = generic::read_device_name(device_path)
            .unwrap_or_else(|| format!("Intel GPU {}", gpu_index));
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
            usage_percent: None,   // Phase 5: perf PMU
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_mock_sysfs(dir: &TempDir, filename: &str, content: &str) {
        fs::write(dir.path().join(filename), content).unwrap();
    }

    #[test]
    fn test_intel_backend_new() {
        let tmp = TempDir::new().unwrap();
        create_mock_sysfs(&tmp, "product_name", "Intel UHD 630");
        let backend = IntelBackend::new(0, tmp.path()).unwrap();
        assert_eq!(backend.gpu_index, 0);
        assert_eq!(backend.gpu_name(), "Intel UHD 630");
    }

    #[test]
    fn test_intel_backend_fallback_name() {
        let tmp = TempDir::new().unwrap();
        let backend = IntelBackend::new(2, tmp.path()).unwrap();
        assert_eq!(backend.gpu_name(), "Intel GPU 2");
    }

    #[test]
    fn test_intel_backend_collect() {
        let tmp = TempDir::new().unwrap();
        create_mock_sysfs(&tmp, "product_name", "Intel Arc A770");

        // Create gt/gt0 directory structure
        fs::create_dir_all(tmp.path().join("gt/gt0")).unwrap();
        create_mock_sysfs(&tmp, "gt/gt0/rps_cur_freq_mhz", "2400");

        let mut backend = IntelBackend::new(1, tmp.path()).unwrap();
        let data = backend.collect().unwrap();

        assert_eq!(data.gpu_index, 1);
        assert_eq!(data.vendor, GpuVendor::Intel);
        assert_eq!(data.name, "Intel Arc A770");
        assert_eq!(data.core_clock_mhz, Some(2400));
        // iGPU has no discrete VRAM
        assert!(data.vram_used_bytes.is_none());
        assert!(data.vram_total_bytes.is_none());
        // Phase 5: utilization not yet implemented
        assert!(data.usage_percent.is_none());
    }

    #[test]
    fn test_intel_backend_collect_no_clock() {
        let tmp = TempDir::new().unwrap();
        create_mock_sysfs(&tmp, "product_name", "Intel GPU");
        // No gt/gt0 or clock file
        let mut backend = IntelBackend::new(0, tmp.path()).unwrap();
        let data = backend.collect().unwrap();
        assert_eq!(data.vendor, GpuVendor::Intel);
        assert!(data.core_clock_mhz.is_none());
    }
}
