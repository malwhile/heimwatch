//! Linux GPU metrics collection via sysfs and vendor APIs.

mod detect;
mod generic;
mod amd;
mod intel;
#[cfg(feature = "nvidia")]
mod nvidia;

use anyhow::Result;
use heimwatch_core::{CollectorEvent, MetricPayload};
use heimwatch_core::metrics::GpuData;
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
