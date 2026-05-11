//! GPU detection via /sys/class/drm/.

use anyhow::{Result, anyhow};
use std::fs;
use std::path::Path;

use super::{GpuBackend, amd, generic, intel};

#[cfg(feature = "nvidia")]
use super::nvidia;

/// Enumerate all GPUs in /sys/class/drm/ and create appropriate backends.
pub fn enumerate_gpus() -> Result<Vec<Box<dyn GpuBackend>>> {
    let drm_path = Path::new("/sys/class/drm");
    if !drm_path.exists() {
        return Err(anyhow!(
            "/sys/class/drm not found; GPU collection unavailable"
        ));
    }

    let mut backends: Vec<Box<dyn GpuBackend>> = Vec::new();
    let entries = fs::read_dir(drm_path)?;

    let mut cards: Vec<_> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            // Only process cardN entries; skip renderD*, controlD*, etc.
            if name_str.starts_with("card")
                && !name_str.contains('-')
                && let Ok(idx) = name_str[4..].parse::<u32>()
            {
                return Some((idx, entry.path()));
            }
            None
        })
        .collect();

    // Sort by card index to maintain consistent ordering
    cards.sort_by_key(|(idx, _)| *idx);

    for (idx, card_path) in cards {
        match detect_and_create_backend(idx, &card_path) {
            Ok(Some(backend)) => backends.push(backend),
            Ok(None) => {
                // No suitable backend for this card; skip
            }
            Err(e) => {
                log::warn!("Failed to detect GPU card{}: {}", idx, e);
            }
        }
    }

    Ok(backends)
}

/// Detect vendor/driver for a single card and create the appropriate backend.
fn detect_and_create_backend(idx: u32, card_path: &Path) -> Result<Option<Box<dyn GpuBackend>>> {
    let device_path = card_path.join("device");
    if !device_path.exists() {
        return Ok(None);
    }

    // Read vendor ID (PCI vendor)
    let vendor_str = fs::read_to_string(device_path.join("vendor"))
        .ok()
        .and_then(|s| s.trim().strip_prefix("0x").map(|v| v.to_string()));

    let vendor_id = if let Some(v) = vendor_str {
        u32::from_str_radix(&v, 16).unwrap_or(0xFFFF)
    } else {
        0xFFFF
    };

    // Try to resolve the driver via the module symlink
    let driver_module = resolve_driver(&device_path);

    // Dispatch based on vendor ID and driver
    let backend: Box<dyn GpuBackend> = match (vendor_id, driver_module.as_deref()) {
        // AMD (0x1002) with amdgpu driver
        (0x1002, Some("amdgpu")) => {
            match amd::AmdBackend::new(idx, &device_path) {
                Ok(b) => Box::new(b),
                Err(e) => {
                    log::warn!("Failed to initialize AMD backend for card{}: {}", idx, e);
                    // Fallback to generic
                    Box::new(generic::GenericBackend::new(idx, &device_path))
                }
            }
        }
        // Intel (0x8086) with i915 or xe driver
        (0x8086, Some("i915" | "xe")) => match intel::IntelBackend::new(idx, &device_path) {
            Ok(b) => Box::new(b),
            Err(e) => {
                log::warn!("Failed to initialize Intel backend for card{}: {}", idx, e);
                Box::new(generic::GenericBackend::new(idx, &device_path))
            }
        },
        // NVIDIA (0x10de) with nvidia driver
        #[cfg(feature = "nvidia")]
        (0x10de, Some("nvidia")) => {
            match nvidia::NvidiaBackend::new(idx, &device_path) {
                Ok(b) => Box::new(b),
                Err(e) => {
                    log::info!("NVIDIA backend unavailable for card{}: {}", idx, e);
                    // Fallback to generic
                    Box::new(generic::GenericBackend::new(idx, &device_path))
                }
            }
        }
        // Fallback for any other vendor/driver
        _ => {
            log::debug!(
                "No specialized backend for card{} (vendor: 0x{:04x}, driver: {:?}); using generic",
                idx,
                vendor_id,
                driver_module
            );
            Box::new(generic::GenericBackend::new(idx, &device_path))
        }
    };

    Ok(Some(backend))
}

/// Resolve the driver module name for a device.
fn resolve_driver(device_path: &Path) -> Option<String> {
    let driver_module = device_path.join("driver").join("module");
    fs::read_link(&driver_module)
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_mock_gpu(
        tmp: &TempDir,
        card_name: &str,
        vendor: &str,
        driver: Option<&str>,
    ) -> std::path::PathBuf {
        let card_path = tmp.path().join(card_name);
        fs::create_dir(&card_path).unwrap();

        let device_path = card_path.join("device");
        fs::create_dir_all(&device_path).unwrap();

        // Write vendor ID
        fs::write(device_path.join("vendor"), vendor).unwrap();

        // Create driver symlink if driver is specified
        if let Some(drv) = driver {
            let driver_path = device_path.join("driver");
            fs::create_dir_all(&driver_path).unwrap();
            // Create a module symlink (point to a fake module directory)
            let module_link = driver_path.join("module");
            #[cfg(unix)]
            {
                use std::os::unix::fs as unix_fs;
                // Create a fake module directory
                let fake_module = tmp.path().join("modules").join(drv);
                fs::create_dir_all(&fake_module).ok();
                unix_fs::symlink(&fake_module, &module_link).ok();
            }
        }

        card_path
    }

    #[test]
    fn test_enumerate_gpus_empty_drm() {
        let tmp = TempDir::new().unwrap();
        // Create an empty /sys/class/drm-like directory
        let result = enumerate_gpus_from_path(tmp.path());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }

    #[test]
    fn test_enumerate_gpus_filters_non_cards() {
        let tmp = TempDir::new().unwrap();

        // Create various entries, only cardN should match
        fs::create_dir(tmp.path().join("renderD128")).unwrap();
        fs::create_dir(tmp.path().join("controlD64")).unwrap();
        create_mock_gpu(&tmp, "card0", "0x8086", Some("i915"));

        let result = enumerate_gpus_from_path(tmp.path());
        assert!(result.is_ok());
        let backends = result.unwrap();
        // Should find card0 but not renderD128 or controlD64
        assert_eq!(backends.len(), 1);
    }

    #[test]
    fn test_detect_vendor_id_parsing() {
        let tmp = TempDir::new().unwrap();
        let card_path = create_mock_gpu(&tmp, "card0", "0x10de\n", Some("nvidia"));
        let vendor_str = fs::read_to_string(card_path.join("device/vendor")).unwrap();
        assert!(vendor_str.trim().starts_with("0x"));
        let vendor_id =
            u32::from_str_radix(vendor_str.trim().trim_start_matches("0x"), 16).unwrap();
        assert_eq!(vendor_id, 0x10de); // NVIDIA
    }

    #[test]
    fn test_resolve_driver_returns_none_when_missing() {
        let tmp = TempDir::new().unwrap();
        let device_path = tmp.path().join("device");
        fs::create_dir_all(&device_path).unwrap();
        let driver = resolve_driver(&device_path);
        assert!(driver.is_none());
    }

    // Helper function for testing with path argument
    fn enumerate_gpus_from_path(drm_path: &Path) -> Result<Vec<Box<dyn GpuBackend>>> {
        let mut backends: Vec<Box<dyn GpuBackend>> = Vec::new();

        if !drm_path.exists() {
            return Err(anyhow::anyhow!("drm path not found"));
        }

        let entries = fs::read_dir(drm_path)?;

        let mut cards: Vec<_> = entries
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with("card")
                    && !name_str.contains('-')
                    && let Ok(idx) = name_str[4..].parse::<u32>()
                {
                    return Some((idx, entry.path()));
                }
                None
            })
            .collect();

        cards.sort_by_key(|(idx, _)| *idx);

        for (idx, card_path) in cards {
            match detect_and_create_backend(idx, &card_path) {
                Ok(Some(backend)) => backends.push(backend),
                Ok(None) => {}
                Err(e) => {
                    log::warn!("Failed to detect GPU card{}: {}", idx, e);
                }
            }
        }

        Ok(backends)
    }
}
