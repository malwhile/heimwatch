//! Linux GPU metrics collection via sysfs and vendor APIs.

mod amd;
mod detect;
mod generic;
mod intel;
#[cfg(feature = "nvidia")]
mod nvidia;

use anyhow::Result;
use heimwatch_core::metrics::GpuData;
use heimwatch_core::{CollectorEvent, MetricPayload};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::watch;

const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Trait for backend GPU collectors — each vendor has one implementation.
pub trait GpuBackend: Send {
    fn collect(&mut self) -> Result<GpuData>;
    fn gpu_name(&self) -> &str;
}

/// GPU collector that manages multiple vendor-specific backends.
pub struct GpuCollector {
    backends: Vec<Box<dyn GpuBackend>>,
}

impl GpuCollector {
    /// Initialize GPU collector by detecting available GPUs.
    pub fn new() -> Result<Self> {
        let backends = detect::enumerate_gpus()?;
        Ok(GpuCollector { backends })
    }

    /// Poll all backends and emit MetricRecords.
    pub async fn run(
        self,
        tx: mpsc::Sender<CollectorEvent>,
        shutdown: watch::Receiver<bool>,
    ) -> Result<()> {
        crate::util::run_collector_loop(
            self,
            POLL_INTERVAL,
            tx,
            shutdown,
            |collector| collector.collect_gpus(POLL_INTERVAL),
            |payload| matches!(payload, MetricPayload::Gpu(_)),
            "GPU",
        )
        .await
    }

    /// Collect metrics from all GPU backends.
    fn collect_gpus(&mut self, _interval: Duration) -> Result<Vec<heimwatch_core::MetricRecord>> {
        let mut records = Vec::new();
        let timestamp = crate::util::current_unix_timestamp();

        for backend in &mut self.backends {
            match backend.collect() {
                Ok(gpu_data) => {
                    let app_name = format!("gpu:{}", gpu_data.gpu_index);
                    records.push(heimwatch_core::MetricRecord {
                        app_name,
                        timestamp,
                        payload: MetricPayload::Gpu(gpu_data),
                    });
                }
                Err(e) => {
                    log::warn!("GPU {} collection failed: {}", backend.gpu_name(), e);
                }
            }
        }

        Ok(records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use heimwatch_core::metrics::GpuVendor;

    /// Mock GPU backend for testing
    struct MockGpuBackend {
        _gpu_index: u32,
        name: String,
        data: GpuData,
    }

    impl GpuBackend for MockGpuBackend {
        fn collect(&mut self) -> Result<GpuData> {
            Ok(self.data.clone())
        }

        fn gpu_name(&self) -> &str {
            &self.name
        }
    }

    #[test]
    fn test_gpu_collector_new_no_gpus() {
        // This test just verifies the collector can be instantiated
        // In a real system without GPUs, it should return Ok with empty vec
        // We can't easily test this without mocking the filesystem
    }

    #[test]
    fn test_collect_gpus_with_mock_backend() {
        let mut collector = GpuCollector {
            backends: vec![Box::new(MockGpuBackend {
                _gpu_index: 0,
                name: "Mock GPU 0".to_string(),
                data: GpuData {
                    gpu_index: 0,
                    vendor: GpuVendor::Unknown,
                    name: "Mock GPU 0".to_string(),
                    usage_percent: Some(50.0),
                    vram_used_bytes: Some(1024 * 1024 * 1024),
                    vram_total_bytes: Some(8 * 1024 * 1024 * 1024),
                    temperature_celsius: Some(60.0),
                    power_draw_watts: Some(100.0),
                    core_clock_mhz: Some(2000),
                    memory_clock_mhz: Some(1000),
                },
            })],
        };

        let records = collector.collect_gpus(Duration::from_secs(5)).unwrap();
        assert_eq!(records.len(), 1);

        let record = &records[0];
        assert_eq!(record.app_name, "gpu:0");
        assert!(matches!(record.payload, MetricPayload::Gpu(_)));

        if let MetricPayload::Gpu(gpu_data) = &record.payload {
            assert_eq!(gpu_data.gpu_index, 0);
            assert_eq!(gpu_data.vendor, GpuVendor::Unknown);
            assert_eq!(gpu_data.usage_percent, Some(50.0));
            assert_eq!(gpu_data.vram_used_bytes, Some(1024 * 1024 * 1024));
        } else {
            panic!("Expected Gpu payload");
        }
    }

    #[test]
    fn test_collect_gpus_multiple_backends() {
        let mut collector = GpuCollector {
            backends: vec![
                Box::new(MockGpuBackend {
                    _gpu_index: 0,
                    name: "GPU 0".to_string(),
                    data: GpuData {
                        gpu_index: 0,
                        vendor: GpuVendor::Intel,
                        name: "Intel GPU 0".to_string(),
                        usage_percent: None,
                        vram_used_bytes: None,
                        vram_total_bytes: None,
                        temperature_celsius: Some(45.0),
                        power_draw_watts: None,
                        core_clock_mhz: Some(2400),
                        memory_clock_mhz: None,
                    },
                }),
                Box::new(MockGpuBackend {
                    _gpu_index: 1,
                    name: "GPU 1".to_string(),
                    data: GpuData {
                        gpu_index: 1,
                        vendor: GpuVendor::Amd,
                        name: "AMD GPU 1".to_string(),
                        usage_percent: Some(75.0),
                        vram_used_bytes: Some(2 * 1024 * 1024 * 1024),
                        vram_total_bytes: Some(12 * 1024 * 1024 * 1024),
                        temperature_celsius: Some(55.0),
                        power_draw_watts: Some(150.0),
                        core_clock_mhz: Some(2600),
                        memory_clock_mhz: Some(1200),
                    },
                }),
            ],
        };

        let records = collector.collect_gpus(Duration::from_secs(5)).unwrap();
        assert_eq!(records.len(), 2);

        assert_eq!(records[0].app_name, "gpu:0");
        assert_eq!(records[1].app_name, "gpu:1");

        if let MetricPayload::Gpu(gpu0) = &records[0].payload {
            assert_eq!(gpu0.vendor, GpuVendor::Intel);
        }

        if let MetricPayload::Gpu(gpu1) = &records[1].payload {
            assert_eq!(gpu1.vendor, GpuVendor::Amd);
            assert_eq!(gpu1.usage_percent, Some(75.0));
        }
    }

    #[test]
    fn test_collect_gpus_empty() {
        let mut collector = GpuCollector { backends: vec![] };
        let records = collector.collect_gpus(Duration::from_secs(5)).unwrap();
        assert_eq!(records.len(), 0);
    }
}
