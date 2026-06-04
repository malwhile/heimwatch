use chrono::{Datelike, NaiveDate};
use heimwatch_core::metrics::{MetricPayload, MetricRecord};

/// Floor a Unix timestamp to the start of its UTC day (00:00:00).
pub fn day_bucket(ts: u64) -> u64 {
    if let Some(dt) = chrono::DateTime::from_timestamp(ts as i64, 0) {
        let date = dt.date_naive();
        if let Some(midnight) = date.and_hms_opt(0, 0, 0) {
            return midnight.and_utc().timestamp() as u64;
        }
    }
    // Fallback: use integer arithmetic (approximate, off by a few seconds at dst boundaries)
    (ts / 86400) * 86400
}

/// Floor a Unix timestamp to the start of its UTC month (1st at 00:00:00).
pub fn month_bucket(ts: u64) -> u64 {
    if let Some(dt) = chrono::DateTime::from_timestamp(ts as i64, 0) {
        let date = dt.date_naive();
        if let Some(first) = NaiveDate::from_ymd_opt(date.year(), date.month(), 1)
            && let Some(midnight) = first.and_hms_opt(0, 0, 0)
        {
            return midnight.and_utc().timestamp() as u64;
        }
    }
    // Fallback: approximately the 1st (off by up to 31 days if month arithmetic is wrong)
    ts
}

/// Floor a Unix timestamp to the start of its UTC year (Jan 1 at 00:00:00).
pub fn year_bucket(ts: u64) -> u64 {
    if let Some(dt) = chrono::DateTime::from_timestamp(ts as i64, 0) {
        let date = dt.date_naive();
        if let Some(jan1) = NaiveDate::from_ymd_opt(date.year(), 1, 1)
            && let Some(midnight) = jan1.and_hms_opt(0, 0, 0)
        {
            return midnight.and_utc().timestamp() as u64;
        }
    }
    // Fallback: approximate year start
    ts
}

/// Aggregate a slice of metric records (all same type and app) into a single record.
///
/// Returns `None` if the slice is empty. Aggregates field-by-field according to type:
/// - Additive fields (bytes, time): sum
/// - Point-in-time fields (percentages, counts): average
/// - Instantaneous readings (temperature, voltage): average
/// - Categorical fields (names, paths, flags): last value
pub fn aggregate_records(records: &[MetricRecord], bucket_ts: u64) -> Option<MetricRecord> {
    if records.is_empty() {
        return None;
    }

    let app_name = records[0].app_name.clone();
    let payload = match &records[0].payload {
        MetricPayload::Net(_) => {
            let (tx_sum, rx_sum, conn_sum) =
                records
                    .iter()
                    .fold((0u64, 0u64, 0u64), |(tx, rx, conn), r| {
                        if let MetricPayload::Net(d) = &r.payload {
                            (
                                tx + d.tx_bytes,
                                rx + d.rx_bytes,
                                conn + d.connections as u64,
                            )
                        } else {
                            (tx, rx, conn)
                        }
                    });
            let conn_avg = (conn_sum as f64 / records.len() as f64).round() as u32;
            MetricPayload::Net(heimwatch_core::metrics::NetworkData {
                tx_bytes: tx_sum,
                rx_bytes: rx_sum,
                connections: conn_avg,
            })
        }
        MetricPayload::Cpu(_) => {
            let (time_sum, usage_sum) = records.iter().fold((0u64, 0.0), |(time, usage), r| {
                if let MetricPayload::Cpu(d) = &r.payload {
                    (time + d.cpu_time_ns, usage + d.cpu_usage_percent as f64)
                } else {
                    (time, usage)
                }
            });
            let usage_avg = (usage_sum / records.len() as f64) as f32;
            MetricPayload::Cpu(heimwatch_core::metrics::CpuData {
                cpu_time_ns: time_sum,
                cpu_usage_percent: usage_avg,
            })
        }
        MetricPayload::Mem(_) => {
            let (rss_sum, vms_sum, swap_sum, proc_sum) =
                records
                    .iter()
                    .fold((0u64, 0u64, 0u64, 0u64), |(rss, vms, swap, proc), r| {
                        if let MetricPayload::Mem(d) = &r.payload {
                            (
                                rss + d.rss_bytes,
                                vms + d.vms_bytes,
                                swap + d.swap_bytes,
                                proc + d.process_count as u64,
                            )
                        } else {
                            (rss, vms, swap, proc)
                        }
                    });
            let n = records.len() as f64;
            MetricPayload::Mem(heimwatch_core::metrics::MemoryData {
                rss_bytes: (rss_sum as f64 / n) as u64,
                vms_bytes: (vms_sum as f64 / n) as u64,
                swap_bytes: (swap_sum as f64 / n) as u64,
                process_count: (proc_sum as f64 / n).round() as u32,
            })
        }
        MetricPayload::Dsk(_) => {
            let (read_sum, write_sum, mount) =
                records
                    .iter()
                    .fold((0u64, 0u64, String::new()), |(read, write, _), r| {
                        if let MetricPayload::Dsk(d) = &r.payload {
                            (
                                read + d.read_bytes,
                                write + d.write_bytes,
                                d.mount_point.clone(),
                            )
                        } else {
                            (read, write, String::new())
                        }
                    });
            MetricPayload::Dsk(heimwatch_core::metrics::DiskData {
                read_bytes: read_sum,
                write_bytes: write_sum,
                mount_point: mount,
            })
        }
        MetricPayload::Foc(_) => {
            let (dur_sum, app_id) = records.iter().fold((0u64, String::new()), |(dur, _), r| {
                if let MetricPayload::Foc(d) = &r.payload {
                    (dur + d.duration_ms, d.app_id.clone())
                } else {
                    (dur, String::new())
                }
            });
            MetricPayload::Foc(heimwatch_core::metrics::FocusData {
                app_id,
                duration_ms: dur_sum,
            })
        }
        MetricPayload::Pwr(_) => {
            let mut watt_sum = 0.0;
            let mut batt_values = Vec::new();
            let mut charging = false;
            let mut rapl_pkg_values = Vec::new();
            let mut rapl_core_values = Vec::new();
            let mut batt_current_values = Vec::new();
            let mut batt_voltage_values = Vec::new();
            let mut freq_ratio_values = Vec::new();
            let mut brightness_values = Vec::new();
            let mut is_wifi = None;

            for r in records {
                if let MetricPayload::Pwr(d) = &r.payload {
                    watt_sum += d.watt_usage as f64;
                    if let Some(b) = d.battery_percent {
                        batt_values.push(b as f64);
                    }
                    charging = d.charging;
                    if let Some(v) = d.rapl_package_watts {
                        rapl_pkg_values.push(v as f64);
                    }
                    if let Some(v) = d.rapl_core_watts {
                        rapl_core_values.push(v as f64);
                    }
                    if let Some(v) = d.battery_current_ua {
                        batt_current_values.push(v);
                    }
                    if let Some(v) = d.battery_voltage_uv {
                        batt_voltage_values.push(v as f64);
                    }
                    if let Some(v) = d.avg_cpu_freq_ratio {
                        freq_ratio_values.push(v as f64);
                    }
                    if let Some(v) = d.display_brightness {
                        brightness_values.push(v as f64);
                    }
                    if d.is_wifi.is_some() {
                        is_wifi = d.is_wifi;
                    }
                }
            }

            let n = records.len() as f64;
            MetricPayload::Pwr(heimwatch_core::metrics::PowerData {
                watt_usage: (watt_sum / n) as f32,
                battery_percent: if batt_values.is_empty() {
                    None
                } else {
                    Some((batt_values.iter().sum::<f64>() / batt_values.len() as f64) as f32)
                },
                charging,
                rapl_package_watts: if rapl_pkg_values.is_empty() {
                    None
                } else {
                    Some(
                        (rapl_pkg_values.iter().sum::<f64>() / rapl_pkg_values.len() as f64) as f32,
                    )
                },
                rapl_core_watts: if rapl_core_values.is_empty() {
                    None
                } else {
                    Some(
                        (rapl_core_values.iter().sum::<f64>() / rapl_core_values.len() as f64)
                            as f32,
                    )
                },
                battery_current_ua: if batt_current_values.is_empty() {
                    None
                } else {
                    Some(batt_current_values.iter().sum::<i64>() / batt_current_values.len() as i64)
                },
                battery_voltage_uv: if batt_voltage_values.is_empty() {
                    None
                } else {
                    Some(
                        (batt_voltage_values.iter().sum::<f64>() / batt_voltage_values.len() as f64)
                            as u64,
                    )
                },
                avg_cpu_freq_ratio: if freq_ratio_values.is_empty() {
                    None
                } else {
                    Some(
                        (freq_ratio_values.iter().sum::<f64>() / freq_ratio_values.len() as f64)
                            as f32,
                    )
                },
                display_brightness: if brightness_values.is_empty() {
                    None
                } else {
                    Some(
                        (brightness_values.iter().sum::<f64>() / brightness_values.len() as f64)
                            as f32,
                    )
                },
                is_wifi,
            })
        }
        MetricPayload::Gpu(_) => {
            let (first, last_usage, name, _vendor) = records.iter().fold(
                (None, Vec::new(), String::new(), None),
                |(first, mut usage, _, _), r| {
                    if let MetricPayload::Gpu(d) = &r.payload {
                        let first = first.or(Some((d.gpu_index, d.vendor)));
                        if let Some(u) = d.usage_percent {
                            usage.push(u as f64);
                        }
                        (first, usage, d.name.clone(), Some(d.vendor))
                    } else {
                        (first, usage, String::new(), None)
                    }
                },
            );

            let (gpu_index, vendor) =
                first.unwrap_or((0, heimwatch_core::metrics::GpuVendor::Unknown));

            let mut vram_used_values = Vec::new();
            let mut vram_total_values = Vec::new();
            let mut temp_values = Vec::new();
            let mut power_values = Vec::new();
            let mut core_clock_values = Vec::new();
            let mut mem_clock_values = Vec::new();

            for r in records {
                if let MetricPayload::Gpu(d) = &r.payload {
                    if let Some(v) = d.vram_used_bytes {
                        vram_used_values.push(v as f64);
                    }
                    if let Some(v) = d.vram_total_bytes {
                        vram_total_values.push(v as f64);
                    }
                    if let Some(v) = d.temperature_celsius {
                        temp_values.push(v as f64);
                    }
                    if let Some(v) = d.power_draw_watts {
                        power_values.push(v as f64);
                    }
                    if let Some(v) = d.core_clock_mhz {
                        core_clock_values.push(v as f64);
                    }
                    if let Some(v) = d.memory_clock_mhz {
                        mem_clock_values.push(v as f64);
                    }
                }
            }

            MetricPayload::Gpu(heimwatch_core::metrics::GpuData {
                gpu_index,
                vendor,
                name,
                usage_percent: if last_usage.is_empty() {
                    None
                } else {
                    Some((last_usage.iter().sum::<f64>() / last_usage.len() as f64) as f32)
                },
                vram_used_bytes: if vram_used_values.is_empty() {
                    None
                } else {
                    Some(
                        (vram_used_values.iter().sum::<f64>() / vram_used_values.len() as f64)
                            as u64,
                    )
                },
                vram_total_bytes: if vram_total_values.is_empty() {
                    None
                } else {
                    Some(
                        (vram_total_values.iter().sum::<f64>() / vram_total_values.len() as f64)
                            as u64,
                    )
                },
                temperature_celsius: if temp_values.is_empty() {
                    None
                } else {
                    Some((temp_values.iter().sum::<f64>() / temp_values.len() as f64) as f32)
                },
                power_draw_watts: if power_values.is_empty() {
                    None
                } else {
                    Some((power_values.iter().sum::<f64>() / power_values.len() as f64) as f32)
                },
                core_clock_mhz: if core_clock_values.is_empty() {
                    None
                } else {
                    Some(
                        (core_clock_values.iter().sum::<f64>() / core_clock_values.len() as f64)
                            as u32,
                    )
                },
                memory_clock_mhz: if mem_clock_values.is_empty() {
                    None
                } else {
                    Some(
                        (mem_clock_values.iter().sum::<f64>() / mem_clock_values.len() as f64)
                            as u32,
                    )
                },
            })
        }
        MetricPayload::GpuProc(_) => {
            let (gpu_index, usage_values) =
                records
                    .iter()
                    .fold((0u32, Vec::new()), |(idx, mut usage), r| {
                        if let MetricPayload::GpuProc(d) = &r.payload {
                            let idx = if idx == 0 { d.gpu_index } else { idx };
                            if let Some(u) = d.usage_percent {
                                usage.push(u as f64);
                            }
                            (idx, usage)
                        } else {
                            (idx, usage)
                        }
                    });

            let mut vram_used_values = Vec::new();
            for r in records {
                if let MetricPayload::GpuProc(d) = &r.payload
                    && let Some(v) = d.vram_used_bytes
                {
                    vram_used_values.push(v as f64);
                }
            }

            MetricPayload::GpuProc(heimwatch_core::metrics::GpuProcessData {
                gpu_index,
                usage_percent: if usage_values.is_empty() {
                    None
                } else {
                    Some((usage_values.iter().sum::<f64>() / usage_values.len() as f64) as f32)
                },
                vram_used_bytes: if vram_used_values.is_empty() {
                    None
                } else {
                    Some(
                        (vram_used_values.iter().sum::<f64>() / vram_used_values.len() as f64)
                            as u64,
                    )
                },
            })
        }
    };

    Some(MetricRecord {
        app_name,
        timestamp: bucket_ts,
        payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_day_bucket() {
        let ts = 1_711_890_000u64; // 2024-03-31 12:00:00 UTC
        let bucket = day_bucket(ts);
        let expected = 1_711_843_200u64; // 2024-03-31 00:00:00 UTC
        assert_eq!(bucket, expected);
    }

    #[test]
    fn test_month_bucket() {
        let ts = 1_711_890_000u64; // 2024-03-31 12:00:00 UTC
        let bucket = month_bucket(ts);
        let expected = 1_709_251_200u64; // 2024-03-01 00:00:00 UTC
        assert_eq!(bucket, expected);
    }

    #[test]
    fn test_year_bucket() {
        let ts = 1_711_890_000u64; // 2024-03-31 12:00:00 UTC
        let bucket = year_bucket(ts);
        let expected = 1_704_067_200u64; // 2024-01-01 00:00:00 UTC
        assert_eq!(bucket, expected);
    }

    #[test]
    fn test_aggregate_network() {
        let records = vec![
            MetricRecord {
                app_name: "app1".to_string(),
                timestamp: 1000,
                payload: MetricPayload::Net(heimwatch_core::metrics::NetworkData {
                    tx_bytes: 100,
                    rx_bytes: 200,
                    connections: 5,
                }),
            },
            MetricRecord {
                app_name: "app1".to_string(),
                timestamp: 2000,
                payload: MetricPayload::Net(heimwatch_core::metrics::NetworkData {
                    tx_bytes: 150,
                    rx_bytes: 250,
                    connections: 7,
                }),
            },
        ];

        let result = aggregate_records(&records, 3000).unwrap();
        assert_eq!(result.app_name, "app1");
        assert_eq!(result.timestamp, 3000);
        if let MetricPayload::Net(d) = result.payload {
            assert_eq!(d.tx_bytes, 250);
            assert_eq!(d.rx_bytes, 450);
            assert_eq!(d.connections, 6); // (5 + 7) / 2 = 6
        } else {
            panic!("Wrong payload type");
        }
    }

    #[test]
    fn test_aggregate_empty() {
        let records: Vec<MetricRecord> = vec![];
        assert!(aggregate_records(&records, 1000).is_none());
    }
}
