use anyhow::Result;
use heimwatch_core::{
    AppFocusStats, AppNetworkStats, AppPowerStats, MetricPayload, MetricRecord, MetricType,
    current_unix_timestamp,
};
use heimwatch_storage::StorageLayer;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct AppCpuStats {
    pub app_name: String,
    pub cpu_usage_percent: f32,
    pub thread_count: u32,
    pub cpu_time_ns: u64,
}

pub struct AppSnapshot {
    pub cpu_series: Vec<u64>,
    pub mem_series: Vec<u64>,
    pub net_tx_series: Vec<u64>,
    pub net_rx_series: Vec<u64>,

    pub current_cpu_pct: f32,
    pub current_ram_mb: u64,
    pub current_net_tx: u64,
    pub current_net_rx: u64,
    pub battery_pct: Option<f32>,
    pub charging: bool,
    pub watt_usage: f32,

    pub top_net_apps: Vec<AppNetworkStats>,
    pub top_focus_apps: Vec<AppFocusStats>,
    pub top_power_apps: Vec<AppPowerStats>,
    pub top_cpu_apps: Vec<AppCpuStats>,
}

pub fn load_snapshot(storage: &StorageLayer, window_secs: u64) -> Result<AppSnapshot> {
    let now = current_unix_timestamp()?;
    let start = now.saturating_sub(window_secs);

    let cpu_records = storage.get_metrics_by_type(MetricType::Cpu, start, now)?;
    let mem_records = storage.get_metrics_by_type(MetricType::Mem, start, now)?;
    let net_records = storage.get_metrics_by_type(MetricType::Net, start, now)?;
    let pwr_records = storage.get_metrics_by_type(MetricType::Pwr, start, now)?;

    let cpu_series = downsample(&cpu_records, |r| {
        if let MetricPayload::Cpu(cpu) = &r.payload {
            Some((cpu.cpu_usage_percent) as u64)
        } else {
            None
        }
    });

    let mem_series = downsample(&mem_records, |r| {
        if let MetricPayload::Mem(mem) = &r.payload {
            Some(mem.rss_bytes / 1_000_000)
        } else {
            None
        }
    });

    let net_tx_series = downsample(&net_records, |r| {
        if let MetricPayload::Net(net) = &r.payload {
            Some(net.tx_bytes)
        } else {
            None
        }
    });

    let net_rx_series = downsample(&net_records, |r| {
        if let MetricPayload::Net(net) = &r.payload {
            Some(net.rx_bytes)
        } else {
            None
        }
    });

    let current_cpu_pct = cpu_records
        .last()
        .and_then(|r| {
            if let MetricPayload::Cpu(cpu) = &r.payload {
                Some(cpu.cpu_usage_percent)
            } else {
                None
            }
        })
        .unwrap_or(0.0);

    let current_ram_mb = mem_records
        .last()
        .and_then(|r| {
            if let MetricPayload::Mem(mem) = &r.payload {
                Some(mem.rss_bytes / 1_000_000)
            } else {
                None
            }
        })
        .unwrap_or(0);

    let (current_net_tx, current_net_rx) = net_records
        .last()
        .and_then(|r| {
            if let MetricPayload::Net(net) = &r.payload {
                Some((net.tx_bytes, net.rx_bytes))
            } else {
                None
            }
        })
        .unwrap_or((0, 0));

    let (battery_pct, charging, watt_usage) = pwr_records
        .last()
        .and_then(|r| {
            if let MetricPayload::Pwr(pwr) = &r.payload {
                Some((pwr.battery_percent, pwr.charging, pwr.watt_usage))
            } else {
                None
            }
        })
        .unwrap_or((None, false, 0.0));

    let top_net_apps = storage.get_top_apps_by_network(start, now, 20)?;
    let top_focus_apps = storage.get_top_apps_by_focus(start, now, 20)?;
    let top_power_apps = storage.get_top_apps_by_power(start, now, None, 20)?;

    let top_cpu_apps = aggregate_cpu_apps(&cpu_records);

    Ok(AppSnapshot {
        cpu_series,
        mem_series,
        net_tx_series,
        net_rx_series,
        current_cpu_pct,
        current_ram_mb,
        current_net_tx,
        current_net_rx,
        battery_pct,
        charging,
        watt_usage,
        top_net_apps,
        top_focus_apps,
        top_power_apps,
        top_cpu_apps,
    })
}

fn downsample<F>(records: &[MetricRecord], extract: F) -> Vec<u64>
where
    F: Fn(&MetricRecord) -> Option<u64>,
{
    const MAX_POINTS: usize = 60;

    let values: Vec<u64> = records.iter().filter_map(extract).collect();

    if values.len() <= MAX_POINTS {
        values
    } else {
        let step = values.len() / MAX_POINTS;
        values.iter().step_by(step).copied().collect()
    }
}

fn aggregate_cpu_apps(cpu_records: &[MetricRecord]) -> Vec<AppCpuStats> {
    #[derive(Clone)]
    struct CpuAggregate {
        percentages: Vec<f32>,
        threads: Vec<u32>,
        times: Vec<u64>,
    }

    let mut app_data: HashMap<String, CpuAggregate> = HashMap::new();

    for record in cpu_records {
        if let MetricPayload::Cpu(cpu) = &record.payload {
            let entry = app_data
                .entry(record.app_name.clone())
                .or_insert(CpuAggregate {
                    percentages: Vec::new(),
                    threads: Vec::new(),
                    times: Vec::new(),
                });
            entry.percentages.push(cpu.cpu_usage_percent);
            entry.threads.push(cpu.thread_count);
            entry.times.push(cpu.cpu_time_ns);
        }
    }

    let mut result: Vec<AppCpuStats> = app_data
        .into_iter()
        .map(|(app_name, data)| {
            let avg_cpu_pct = data.percentages.iter().sum::<f32>() / data.percentages.len() as f32;
            let avg_threads = (data.threads.iter().sum::<u32>() as f64 / data.threads.len() as f64)
                .round() as u32;
            let avg_cpu_time =
                (data.times.iter().sum::<u64>() as f64 / data.times.len() as f64).round() as u64;

            AppCpuStats {
                app_name,
                cpu_usage_percent: avg_cpu_pct,
                thread_count: avg_threads,
                cpu_time_ns: avg_cpu_time,
            }
        })
        .collect();

    result.sort_by(|a, b| {
        b.cpu_usage_percent
            .partial_cmp(&a.cpu_usage_percent)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    result.truncate(20);

    result
}
