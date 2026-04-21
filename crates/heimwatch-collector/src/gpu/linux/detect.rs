//! GPU detection via /sys/class/drm/.

use anyhow::{anyhow, Result};
use std::fs;
use std::path::Path;

use super::{generic, amd, intel, GpuBackend};

#[cfg(feature = "nvidia")]
use super::nvidia;

/// Enumerate all GPUs in /sys/class/drm/ and create appropriate backends.
pub fn enumerate_gpus() -> Result<Vec<Box<dyn GpuBackend>>> {
    let drm_path = Path::new("/sys/class/drm");
    if !drm_path.exists() {
        return Err(anyhow!("/sys/class/drm not found; GPU collection unavailable"));
    }

    let mut backends: Vec<Box<dyn GpuBackend>> = Vec::new();
    let entries = fs::read_dir(drm_path)?;

    let mut cards: Vec<_> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            // Only process cardN entries; skip renderD*, controlD*, etc.
            if name_str.starts_with("card") && !name_str.contains('-') {
                if let Ok(idx) = name_str[4..].parse::<u32>() {
                    return Some((idx, entry.path()));
                }
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
        (0x8086, Some("i915" | "xe")) => {
            match intel::IntelBackend::new(idx, &device_path) {
                Ok(b) => Box::new(b),
                Err(e) => {
                    log::warn!("Failed to initialize Intel backend for card{}: {}", idx, e);
                    Box::new(generic::GenericBackend::new(idx, &device_path))
                }
            }
        }
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
