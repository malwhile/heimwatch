//! Generic GPU backend (hwmon only) and shared sysfs utilities.

use anyhow::Result;
use heimwatch_core::metrics::{GpuData, GpuVendor};
use std::fs;
use std::path::{Path, PathBuf};

use super::GpuBackend;

/// Read a u64 from a sysfs file.
pub fn read_sysfs_u64(path: &Path) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse::<u64>().ok()
}

/// Read a string from a sysfs file.
pub fn read_sysfs_string(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// Find the hwmon path linked from a DRM device.
pub fn find_hwmon_path(device_path: &Path) -> Option<PathBuf> {
    let hwmon_dir = device_path.join("hwmon");
    if hwmon_dir.exists()
        && let Ok(entries) = fs::read_dir(&hwmon_dir)
    {
        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with("hwmon") {
                return Some(entry.path());
            }
        }
    }
    None
}

/// Read temperature from hwmon (temp1_input in millidegrees Celsius).
pub fn read_hwmon_temp_celsius(hwmon: &Path) -> Option<f32> {
    read_sysfs_u64(&hwmon.join("temp1_input")).map(|millidegrees| millidegrees as f32 / 1000.0)
}

/// Read power from hwmon (power1_average in microwatts).
pub fn read_hwmon_power_watts(hwmon: &Path) -> Option<f32> {
    read_sysfs_u64(&hwmon.join("power1_average")).map(|microwatts| microwatts as f32 / 1_000_000.0)
}

/// Generic GPU backend — reads only hwmon (temperature, power).
/// Used as fallback when no vendor-specific backend is available.
pub struct GenericBackend {
    gpu_index: u32,
    #[allow(dead_code)]
    device_path: PathBuf,
    pci_address: String,
    hwmon_path: Option<PathBuf>,
    name: String,
}

impl GenericBackend {
    pub fn new(gpu_index: u32, device_path: &Path) -> Self {
        let hwmon_path = find_hwmon_path(device_path);
        let name = read_device_name(device_path).unwrap_or_else(|| format!("GPU {}", gpu_index));
        let pci_address = read_pci_slot_name(device_path).unwrap_or_default();
        GenericBackend {
            gpu_index,
            device_path: device_path.to_path_buf(),
            pci_address,
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

    fn pci_address(&self) -> &str {
        &self.pci_address
    }

    fn gpu_index(&self) -> u32 {
        self.gpu_index
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

/// Read the PCI slot name (e.g., "0000:c5:00.0") from a device's uevent file.
pub fn read_pci_slot_name(device_path: &Path) -> Option<String> {
    let uevent = fs::read_to_string(device_path.join("uevent")).ok()?;
    uevent
        .lines()
        .find(|l| l.starts_with("PCI_SLOT_NAME="))
        .and_then(|l| l.split_once('='))
        .map(|(_, v)| v.trim().to_string())
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
    fn test_read_sysfs_u64_valid() {
        let tmp = TempDir::new().unwrap();
        create_mock_sysfs(&tmp, "test.txt", "42");
        let result = read_sysfs_u64(&tmp.path().join("test.txt"));
        assert_eq!(result, Some(42));
    }

    #[test]
    fn test_read_sysfs_u64_with_whitespace() {
        let tmp = TempDir::new().unwrap();
        create_mock_sysfs(&tmp, "test.txt", "  100  \n");
        let result = read_sysfs_u64(&tmp.path().join("test.txt"));
        assert_eq!(result, Some(100));
    }

    #[test]
    fn test_read_sysfs_u64_invalid() {
        let tmp = TempDir::new().unwrap();
        create_mock_sysfs(&tmp, "test.txt", "not_a_number");
        let result = read_sysfs_u64(&tmp.path().join("test.txt"));
        assert_eq!(result, None);
    }

    #[test]
    fn test_read_sysfs_u64_nonexistent() {
        let tmp = TempDir::new().unwrap();
        let result = read_sysfs_u64(&tmp.path().join("nonexistent.txt"));
        assert_eq!(result, None);
    }

    #[test]
    fn test_read_sysfs_string_valid() {
        let tmp = TempDir::new().unwrap();
        create_mock_sysfs(&tmp, "test.txt", "AMD RX 7900");
        let result = read_sysfs_string(&tmp.path().join("test.txt"));
        assert_eq!(result, Some("AMD RX 7900".to_string()));
    }

    #[test]
    fn test_read_sysfs_string_trims_whitespace() {
        let tmp = TempDir::new().unwrap();
        create_mock_sysfs(&tmp, "test.txt", "  GPU NAME  \n");
        let result = read_sysfs_string(&tmp.path().join("test.txt"));
        assert_eq!(result, Some("GPU NAME".to_string()));
    }

    #[test]
    fn test_read_hwmon_temp_celsius() {
        let hwmon = TempDir::new().unwrap();
        create_mock_sysfs(&hwmon, "temp1_input", "45000");
        let result = read_hwmon_temp_celsius(hwmon.path());
        assert_eq!(result, Some(45.0));
    }

    #[test]
    fn test_read_hwmon_power_watts() {
        let hwmon = TempDir::new().unwrap();
        create_mock_sysfs(&hwmon, "power1_average", "150000000");
        let result = read_hwmon_power_watts(hwmon.path());
        assert_eq!(result, Some(150.0));
    }

    #[test]
    fn test_find_hwmon_path_exists() {
        let tmp = TempDir::new().unwrap();
        let hwmon_dir = tmp.path().join("hwmon");
        fs::create_dir(&hwmon_dir).unwrap();
        fs::create_dir(hwmon_dir.join("hwmon0")).unwrap();
        let result = find_hwmon_path(tmp.path());
        assert!(result.is_some());
        assert!(result.unwrap().ends_with("hwmon0"));
    }

    #[test]
    fn test_find_hwmon_path_nonexistent() {
        let tmp = TempDir::new().unwrap();
        let result = find_hwmon_path(tmp.path());
        assert_eq!(result, None);
    }

    #[test]
    fn test_generic_backend_new() {
        let tmp = TempDir::new().unwrap();
        create_mock_sysfs(&tmp, "product_name", "Test GPU");
        let backend = GenericBackend::new(0, tmp.path());
        assert_eq!(backend.gpu_index, 0);
        assert_eq!(backend.gpu_name(), "Test GPU");
    }

    #[test]
    fn test_generic_backend_fallback_name() {
        let tmp = TempDir::new().unwrap();
        let backend = GenericBackend::new(5, tmp.path());
        assert_eq!(backend.gpu_name(), "GPU 5");
    }

    #[test]
    fn test_generic_backend_collect() {
        let tmp = TempDir::new().unwrap();
        create_mock_sysfs(&tmp, "product_name", "Generic GPU");
        let mut backend = GenericBackend::new(1, tmp.path());
        let data = backend.collect().unwrap();
        assert_eq!(data.gpu_index, 1);
        assert_eq!(data.vendor, GpuVendor::Unknown);
        assert_eq!(data.name, "Generic GPU");
        // No hwmon, so these should be None
        assert!(data.temperature_celsius.is_none());
        assert!(data.power_draw_watts.is_none());
    }
}
