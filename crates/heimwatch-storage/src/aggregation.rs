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
///
/// Uses chrono for correct date arithmetic. Logs a warning if fallback arithmetic
/// is used, as this may indicate unusual timestamps.
pub fn month_bucket(ts: u64) -> u64 {
    if let Some(dt) = chrono::DateTime::from_timestamp(ts as i64, 0) {
        let date = dt.date_naive();
        if let Some(first) = NaiveDate::from_ymd_opt(date.year(), date.month(), 1)
            && let Some(midnight) = first.and_hms_opt(0, 0, 0)
        {
            return midnight.and_utc().timestamp() as u64;
        }
    }
    // Fallback: arithmetic approximation (conservative, may be off by a few days)
    log::warn!(
        "month_bucket: chrono date conversion failed for ts={}, using fallback arithmetic",
        ts
    );
    let days_since_epoch = ts / 86_400;
    let approx_days_per_month = 30u64;
    (days_since_epoch / approx_days_per_month) * approx_days_per_month * 86_400
}

/// Floor a Unix timestamp to the start of its UTC year (Jan 1 at 00:00:00).
///
/// Uses chrono for correct date arithmetic. Logs a warning if fallback arithmetic
/// is used, as this may indicate unusual timestamps.
pub fn year_bucket(ts: u64) -> u64 {
    if let Some(dt) = chrono::DateTime::from_timestamp(ts as i64, 0) {
        let date = dt.date_naive();
        if let Some(jan1) = NaiveDate::from_ymd_opt(date.year(), 1, 1)
            && let Some(midnight) = jan1.and_hms_opt(0, 0, 0)
        {
            return midnight.and_utc().timestamp() as u64;
        }
    }
    // Fallback: arithmetic approximation (conservative, may be off)
    log::warn!(
        "year_bucket: chrono date conversion failed for ts={}, using fallback arithmetic",
        ts
    );
    let days_since_epoch = ts / 86_400;
    let approx_days_per_year = 365u64;
    (days_since_epoch / approx_days_per_year) * approx_days_per_year * 86_400
}

/// Subtract months from a Unix timestamp using chrono, with safe fallback.
///
/// Returns a timestamp representing the start of the month `months` ago.
/// Logs a warning and uses conservative arithmetic fallback if chrono conversion fails.
pub fn subtract_months(ts: u64, months: u32) -> u64 {
    if let Some(dt) = chrono::DateTime::from_timestamp(ts as i64, 0) {
        let date = dt.date_naive();
        if let Some(new_date) = date.checked_sub_months(chrono::Months::new(months))
            && let Some(midnight) = new_date.and_hms_opt(0, 0, 0)
        {
            return midnight.and_utc().timestamp() as u64;
        }
    }
    // Fallback: conservative arithmetic (30 days per month, overestimates)
    log::warn!(
        "subtract_months: chrono date arithmetic failed for ts={} minus {} months, using fallback",
        ts,
        months
    );
    ts.saturating_sub(months as u64 * 30 * 86_400)
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

    #[test]
    fn test_aggregate_cpu_sums_time_averages_usage() {
        let records = vec![
            MetricRecord {
                app_name: "app".to_string(),
                timestamp: 1000,
                payload: MetricPayload::Cpu(heimwatch_core::metrics::CpuData {
                    cpu_time_ns: 1_000_000_000,
                    cpu_usage_percent: 10.0,
                }),
            },
            MetricRecord {
                app_name: "app".to_string(),
                timestamp: 2000,
                payload: MetricPayload::Cpu(heimwatch_core::metrics::CpuData {
                    cpu_time_ns: 3_000_000_000,
                    cpu_usage_percent: 30.0,
                }),
            },
        ];

        let result = aggregate_records(&records, 3000).unwrap();
        if let MetricPayload::Cpu(d) = result.payload {
            assert_eq!(d.cpu_time_ns, 4_000_000_000); // sum
            assert!((d.cpu_usage_percent - 20.0).abs() < 0.01); // average
        } else {
            panic!("Wrong payload type");
        }
    }

    #[test]
    fn test_aggregate_power_averages_options() {
        let records = vec![
            MetricRecord {
                app_name: "app".to_string(),
                timestamp: 1000,
                payload: MetricPayload::Pwr(heimwatch_core::metrics::PowerData {
                    watt_usage: 10.0,
                    battery_percent: Some(80.0),
                    charging: false,
                    rapl_package_watts: Some(5.0),
                    rapl_core_watts: None,
                    battery_current_ua: None,
                    battery_voltage_uv: None,
                    avg_cpu_freq_ratio: None,
                    display_brightness: Some(0.5),
                    is_wifi: Some(true),
                }),
            },
            MetricRecord {
                app_name: "app".to_string(),
                timestamp: 2000,
                payload: MetricPayload::Pwr(heimwatch_core::metrics::PowerData {
                    watt_usage: 20.0,
                    battery_percent: Some(70.0),
                    charging: true,
                    rapl_package_watts: Some(15.0),
                    rapl_core_watts: Some(8.0),
                    battery_current_ua: Some(1000),
                    battery_voltage_uv: Some(5_000_000),
                    avg_cpu_freq_ratio: Some(0.8),
                    display_brightness: Some(0.7),
                    is_wifi: Some(false),
                }),
            },
        ];

        let result = aggregate_records(&records, 3000).unwrap();
        if let MetricPayload::Pwr(d) = result.payload {
            assert!((d.watt_usage - 15.0).abs() < 0.01); // average
            assert!((d.battery_percent.unwrap() - 75.0).abs() < 0.01); // average of Some
            assert_eq!(d.charging, true); // last value
            assert!((d.rapl_package_watts.unwrap() - 10.0).abs() < 0.01); // average
            assert!(d.rapl_core_watts.is_some()); // Some present
            assert!(d.battery_current_ua.is_some());
            assert!(d.is_wifi == Some(false)); // last value
        } else {
            panic!("Wrong payload type");
        }
    }

    #[test]
    fn test_aggregate_memory_averages_counts() {
        let records = vec![
            MetricRecord {
                app_name: "app".to_string(),
                timestamp: 1000,
                payload: MetricPayload::Mem(heimwatch_core::metrics::MemoryData {
                    rss_bytes: 1_000_000,
                    vms_bytes: 2_000_000,
                    swap_bytes: 100_000,
                    process_count: 5,
                }),
            },
            MetricRecord {
                app_name: "app".to_string(),
                timestamp: 2000,
                payload: MetricPayload::Mem(heimwatch_core::metrics::MemoryData {
                    rss_bytes: 3_000_000,
                    vms_bytes: 4_000_000,
                    swap_bytes: 300_000,
                    process_count: 7,
                }),
            },
        ];

        let result = aggregate_records(&records, 3000).unwrap();
        if let MetricPayload::Mem(d) = result.payload {
            assert_eq!(d.rss_bytes, 2_000_000); // average: (1M + 3M) / 2
            assert_eq!(d.vms_bytes, 3_000_000); // average: (2M + 4M) / 2
            assert_eq!(d.swap_bytes, 200_000); // average: (100k + 300k) / 2
            assert_eq!(d.process_count, 6); // average rounded: (5 + 7) / 2 = 6
        } else {
            panic!("Wrong payload type");
        }
    }

    #[test]
    fn test_aggregate_disk_sums_bytes_takes_last_mount() {
        let records = vec![
            MetricRecord {
                app_name: "app".to_string(),
                timestamp: 1000,
                payload: MetricPayload::Dsk(heimwatch_core::metrics::DiskData {
                    read_bytes: 1000,
                    write_bytes: 2000,
                    mount_point: "/mnt/old".to_string(),
                }),
            },
            MetricRecord {
                app_name: "app".to_string(),
                timestamp: 2000,
                payload: MetricPayload::Dsk(heimwatch_core::metrics::DiskData {
                    read_bytes: 3000,
                    write_bytes: 4000,
                    mount_point: "/mnt/new".to_string(),
                }),
            },
        ];

        let result = aggregate_records(&records, 3000).unwrap();
        if let MetricPayload::Dsk(d) = result.payload {
            assert_eq!(d.read_bytes, 4000); // sum
            assert_eq!(d.write_bytes, 6000); // sum
            assert_eq!(d.mount_point, "/mnt/new"); // last value
        } else {
            panic!("Wrong payload type");
        }
    }

    #[test]
    fn test_subtract_months_basic() {
        let now = 1_717_334_400u64; // 2024-06-02 00:00:00 UTC
        let one_month_ago = subtract_months(now, 1);
        let two_months_ago = subtract_months(now, 2);

        // Verify subtract_months works correctly
        assert!(one_month_ago < now);
        assert!(two_months_ago < one_month_ago);
    }

    #[test]
    fn test_bucket_functions_monotonic() {
        // Verify bucket functions are monotonic: if ts1 < ts2, bucket(ts1) <= bucket(ts2)
        let ts1 = 1_000_000u64;
        let ts2 = 2_000_000u64;
        let ts3 = 3_000_000u64;

        assert!(day_bucket(ts1) <= day_bucket(ts2));
        assert!(day_bucket(ts2) <= day_bucket(ts3));
        assert!(month_bucket(ts1) <= month_bucket(ts2));
        assert!(month_bucket(ts2) <= month_bucket(ts3));
        assert!(year_bucket(ts1) <= year_bucket(ts2));
        assert!(year_bucket(ts2) <= year_bucket(ts3));
    }

    #[test]
    fn test_day_bucket_midnight_boundary() {
        // Test that records on the same day map to the same bucket
        let day_start = 1_704_067_200u64; // 2024-01-01 00:00:00 UTC
        let day_end = 1_704_153_599u64; // 2024-01-01 23:59:59 UTC
        let next_day = 1_704_153_600u64; // 2024-01-02 00:00:00 UTC

        assert_eq!(day_bucket(day_start), day_start);
        assert_eq!(day_bucket(day_end), day_start); // Same day bucket
        assert!(day_bucket(next_day) > day_bucket(day_start)); // Different day
    }
}
