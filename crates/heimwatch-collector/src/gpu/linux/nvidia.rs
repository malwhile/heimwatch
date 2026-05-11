//! NVIDIA GPU backend via NVML (feature-gated).

use anyhow::{Result, anyhow};
use heimwatch_core::metrics::{GpuData, GpuVendor};
use nvml_wrapper::Nvml;
use nvml_wrapper::enum_wrappers::device::{Clock, ClockId, TemperatureSensor};
use std::path::Path;

use super::GpuBackend;

/// NVIDIA GPU backend — reads metrics via NVML library.
/// Requires: nvml-wrapper crate (optional feature); NVML library at runtime.
pub struct NvidiaBackend {
    gpu_index: u32,
    nvml_device: nvml_wrapper::Device<'static>,
    name: String,
}

impl NvidiaBackend {
    pub fn new(gpu_index: u32, _device_path: &Path) -> Result<Self> {
        // Initialize NVML (requires libnvidia-ml.so at runtime).
        // This will fail gracefully if NVML is not installed.
        let nvml = Nvml::init().map_err(|e| {
            anyhow!(
                "NVML initialization failed (NVIDIA driver/library not available): {}",
                e
            )
        })?;

        // Leak the NVML instance to extend its lifetime to 'static.
        // This is safe because NVML is process-singleton and we never unload it.
        let nvml = Box::leak(Box::new(nvml));

        // Get device by index.
        let nvml_device = nvml
            .device_by_index(gpu_index)
            .map_err(|e| anyhow!("Failed to get NVML device {}: {}", gpu_index, e))?;

        let name = nvml_device
            .name()
            .unwrap_or_else(|_| format!("NVIDIA GPU {}", gpu_index))
            .to_string();

        Ok(NvidiaBackend {
            gpu_index,
            nvml_device,
            name,
        })
    }
}

impl GpuBackend for NvidiaBackend {
    fn collect(&mut self) -> Result<GpuData> {
        let mut data = GpuData {
            gpu_index: self.gpu_index,
            vendor: GpuVendor::Nvidia,
            name: self.name.clone(),
            usage_percent: None,
            vram_used_bytes: None,
            vram_total_bytes: None,
            temperature_celsius: None,
            power_draw_watts: None,
            core_clock_mhz: None,
            memory_clock_mhz: None,
        };

        // GPU utilization (percentage)
        if let Ok(util) = self.nvml_device.utilization_rates() {
            data.usage_percent = Some(util.gpu as f32);
        }

        // VRAM
        if let Ok(mem_info) = self.nvml_device.memory_info() {
            data.vram_used_bytes = Some(mem_info.used);
            data.vram_total_bytes = Some(mem_info.total);
        }

        // Temperature (Celsius)
        if let Ok(temp) = self.nvml_device.temperature(TemperatureSensor::Gpu) {
            data.temperature_celsius = Some(temp as f32);
        }

        // Power draw (milliwatts -> watts)
        if let Ok(power_mw) = self.nvml_device.power_usage() {
            data.power_draw_watts = Some(power_mw as f32 / 1000.0);
        }

        // Clock speeds (MHz)
        if let Ok(core_clock) = self.nvml_device.clock(Clock::Graphics, ClockId::Current) {
            data.core_clock_mhz = Some(core_clock);
        }
        if let Ok(mem_clock) = self.nvml_device.clock(Clock::Memory, ClockId::Current) {
            data.memory_clock_mhz = Some(mem_clock);
        }

        Ok(data)
    }

    fn gpu_name(&self) -> &str {
        &self.name
    }
}
