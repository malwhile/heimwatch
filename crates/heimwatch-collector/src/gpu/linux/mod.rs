//! Linux GPU metrics collection via sysfs and vendor APIs.

mod amd;
mod detect;
mod fdinfo;
mod generic;
mod intel;
#[cfg(feature = "nvidia")]
mod nvidia;

use anyhow::Result;
use heimwatch_core::metrics::{GpuData, GpuProcessData};
use heimwatch_core::{CollectorEvent, MetricPayload};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::watch;

const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Trait for backend GPU collectors — each vendor has one implementation.
pub trait GpuBackend: Send {
    fn collect(&mut self) -> Result<GpuData>;
    fn gpu_name(&self) -> &str;
    fn pci_address(&self) -> &str;
    fn gpu_index(&self) -> u32;
}

/// Key for fdinfo state tracking: (pid, pdev).
type FdinfoKey = (u32, String);

/// Snapshot of per-process GPU stats for delta calculation.
#[derive(Default, Clone, Copy)]
struct FdinfoSnapshot {
    engine_time: u64,
    total_cycles: u64,
    #[allow(dead_code)]
    is_cycles_schema: bool,
}

/// GPU collector that manages multiple vendor-specific backends.
pub struct GpuCollector {
    backends: Vec<Box<dyn GpuBackend>>,
    /// Previous fdinfo snapshot per (pid, pdev) — survives process restarts poorly,
    /// but since we key by pid the worst case is a 0-usage interval on PID reuse.
    prev_fdinfo: HashMap<FdinfoKey, FdinfoSnapshot>,
}

impl GpuCollector {
    /// Initialize GPU collector by detecting available GPUs.
    pub fn new() -> Result<Self> {
        let backends = detect::enumerate_gpus()?;
        Ok(GpuCollector {
            backends,
            prev_fdinfo: HashMap::new(),
        })
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
            |payload| matches!(payload, MetricPayload::Gpu(_) | MetricPayload::GpuProc(_)),
            "GPU",
        )
        .await
    }

    /// Collect metrics from all GPU backends and per-process GPU usage.
    pub fn collect_gpus(
        &mut self,
        interval: Duration,
    ) -> Result<Vec<heimwatch_core::MetricRecord>> {
        let mut records = Vec::new();
        let timestamp = crate::util::current_unix_timestamp();
        let interval_ns = interval.as_nanos() as u64;

        // --- 1. Emit per-GPU aggregate records ---
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

        // --- 2. Per-process records via fdinfo scanning ---
        if !self.backends.is_empty() {
            let known_pdevs: std::collections::HashSet<String> = self
                .backends
                .iter()
                .map(|b| b.pci_address().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            if !known_pdevs.is_empty() {
                let pdev_to_index: HashMap<String, u32> = self
                    .backends
                    .iter()
                    .filter_map(|b| {
                        let addr = b.pci_address().to_string();
                        if addr.is_empty() {
                            None
                        } else {
                            Some((addr, b.gpu_index()))
                        }
                    })
                    .collect();

                let process_stats = fdinfo::scan_proc_fdinfo(&known_pdevs);

                // Build current snapshot map
                let mut current_fdinfo: HashMap<FdinfoKey, FdinfoSnapshot> = HashMap::new();

                for (pid, _app_name, stats) in &process_stats {
                    let key = (*pid, stats.pdev.clone());
                    current_fdinfo.insert(
                        key,
                        FdinfoSnapshot {
                            engine_time: stats.engine_time,
                            total_cycles: stats.total_cycles,
                            is_cycles_schema: stats.is_cycles_schema,
                        },
                    );
                }

                // Aggregate by (app_name, pdev)
                #[derive(Default)]
                struct AppGpuAgg {
                    gpu_index: u32,
                    usage_sum: f32,
                    usage_count: u32,
                    vram_bytes: u64,
                }

                let mut by_app_pdev: HashMap<(String, String), AppGpuAgg> = HashMap::new();

                for (pid, app_name, stats) in process_stats {
                    let prev = self
                        .prev_fdinfo
                        .get(&(pid, stats.pdev.clone()))
                        .copied()
                        .unwrap_or_default();

                    // Delta computation
                    let usage_percent = if stats.is_cycles_schema {
                        // Intel/NVIDIA: delta_cycles / delta_total_cycles * 100
                        let delta_cycles = stats.engine_time.saturating_sub(prev.engine_time);
                        let delta_total = stats.total_cycles.saturating_sub(prev.total_cycles);
                        if delta_total > 0 {
                            Some((delta_cycles as f64 / delta_total as f64 * 100.0) as f32)
                        } else {
                            None
                        }
                    } else {
                        // AMD: delta_ns / interval_ns * 100
                        let delta_ns = stats.engine_time.saturating_sub(prev.engine_time);
                        if interval_ns > 0 && delta_ns > 0 {
                            Some((delta_ns as f64 / interval_ns as f64 * 100.0) as f32)
                        } else {
                            None
                        }
                    };

                    let agg = by_app_pdev
                        .entry((app_name, stats.pdev.clone()))
                        .or_default();
                    if let Some(usage) = usage_percent {
                        agg.usage_sum += usage;
                        agg.usage_count += 1;
                    }
                    agg.vram_bytes = agg.vram_bytes.saturating_add(stats.vram_bytes);
                    if let Some(&idx) = pdev_to_index.get(&stats.pdev) {
                        agg.gpu_index = idx;
                    }
                }

                // Emit one GpuProc record per (app_name, pdev)
                for ((app_name, _pdev), agg) in by_app_pdev {
                    let usage = if agg.usage_count > 0 {
                        Some(agg.usage_sum)
                    } else {
                        None
                    };
                    records.push(heimwatch_core::MetricRecord {
                        app_name,
                        timestamp,
                        payload: MetricPayload::GpuProc(GpuProcessData {
                            gpu_index: agg.gpu_index,
                            usage_percent: usage,
                            vram_used_bytes: if agg.vram_bytes > 0 {
                                Some(agg.vram_bytes)
                            } else {
                                None
                            },
                        }),
                    });
                }

                // Update delta state
                self.prev_fdinfo = current_fdinfo;
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

        fn pci_address(&self) -> &str {
            ""
        }

        fn gpu_index(&self) -> u32 {
            self._gpu_index
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
            prev_fdinfo: HashMap::new(),
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
            prev_fdinfo: HashMap::new(),
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
        let mut collector = GpuCollector {
            backends: vec![],
            prev_fdinfo: HashMap::new(),
        };
        let records = collector.collect_gpus(Duration::from_secs(5)).unwrap();
        assert_eq!(records.len(), 0);
    }
}
