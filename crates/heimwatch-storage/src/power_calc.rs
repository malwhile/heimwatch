//! Power score calculation logic.
//!
//! This module contains the pure calculation logic for computing per-app power consumption
//! statistics. It is independent of storage and can be tested and swapped independently
//! (e.g., for Approach A fixed weights vs. Approach B RAPL-calibrated in Phase 4).

use heimwatch_core::AppPowerStats;
use std::collections::HashMap;

/// Aggregated metrics for a single app across a time range.
#[derive(Default)]
pub struct AppMetrics {
    pub cpu_pcts: Vec<f32>,
    pub gpu_pcts: Vec<f32>,
    pub focus_ms: u64,
    pub disk_bytes: u64,
    pub net_bytes: u64,
    pub mem_rss: Vec<u64>,
}

/// Intermediate score components for computing contributions.
#[derive(Default)]
struct ScoreComponents {
    cpu: f32,
    gpu: f32,
    display: f32,
    disk: f32,
    net: f32,
    mem: f32,
}

/// Compute per-app power consumption statistics using fixed-weight approach.
///
/// # Arguments
/// * `app_metrics` - Pre-aggregated metrics per app (CPU %, GPU %, focus time, disk/net/memory bytes)
/// * `window_ms` - Query window duration in milliseconds (used to normalize focus time)
/// * `on_battery_filter` - Optional battery state filter. If Some, only included timestamps are counted.
///
/// # Approach A (Fixed Weights)
/// - CPU: 40% (or RAPL-calibrated in Approach B / Phase 4)
/// - GPU: 20%
/// - Display (focus time): 15%
/// - Disk I/O: 10%
/// - Network: 10%
/// - Memory: 5%
///
/// Returns apps sorted descending by `power_pct`.
pub fn compute_power_stats(
    app_metrics: HashMap<String, AppMetrics>,
    window_ms: u64,
) -> Vec<AppPowerStats> {
    if window_ms == 0 {
        return Vec::new();
    }

    // Compute normalization factors
    let max_disk_bytes = app_metrics
        .values()
        .map(|m| m.disk_bytes)
        .max()
        .unwrap_or(1);
    let max_net_bytes = app_metrics.values().map(|m| m.net_bytes).max().unwrap_or(1);
    let total_mem_rss: u64 = app_metrics.values().flat_map(|m| &m.mem_rss).sum();

    // Compute power scores (with intermediate components for contributions)
    let power_stats: Vec<(AppPowerStats, ScoreComponents)> = app_metrics
        .into_iter()
        .map(|(app_name, metrics)| {
            let cpu_pct_avg = if metrics.cpu_pcts.is_empty() {
                0.0
            } else {
                metrics.cpu_pcts.iter().sum::<f32>() / metrics.cpu_pcts.len() as f32
            };

            let gpu_pct_max = metrics.gpu_pcts.iter().cloned().fold(0.0, f32::max);

            let focus_fraction = (metrics.focus_ms as f32 / window_ms as f32) * 100.0;
            let focus_fraction = focus_fraction.clamp(0.0, 100.0);

            let disk_normalized = if max_disk_bytes > 0 {
                ((metrics.disk_bytes as f32 + 1.0).log2() / (max_disk_bytes as f32 + 1.0).log2())
                    * 100.0
            } else {
                0.0
            };

            let net_normalized = if max_net_bytes > 0 {
                ((metrics.net_bytes as f32 + 1.0).log2() / (max_net_bytes as f32 + 1.0).log2())
                    * 100.0
            } else {
                0.0
            };

            let mem_fraction = if total_mem_rss > 0 {
                let mem_rss_avg = if metrics.mem_rss.is_empty() {
                    0
                } else {
                    metrics.mem_rss.iter().sum::<u64>() / metrics.mem_rss.len() as u64
                };
                (mem_rss_avg as f32 / total_mem_rss as f32) * 100.0
            } else {
                0.0
            };

            // Compute weighted components (Approach A: fixed weights)
            let components = ScoreComponents {
                cpu: 0.40 * cpu_pct_avg,
                gpu: 0.20 * gpu_pct_max,
                display: 0.15 * focus_fraction,
                disk: 0.10 * disk_normalized,
                net: 0.10 * net_normalized,
                mem: 0.05 * mem_fraction,
            };

            let power_score = components.cpu
                + components.gpu
                + components.display
                + components.disk
                + components.net
                + components.mem;

            let stat = AppPowerStats {
                app_name,
                power_pct: 0.0, // Normalized later
                power_score,
                on_battery: false,
                cpu_contribution: 0.0,
                gpu_contribution: 0.0,
                display_contribution: 0.0,
                disk_contribution: 0.0,
                net_contribution: 0.0,
                mem_contribution: 0.0,
            };

            (stat, components)
        })
        .collect();

    // Normalize scores to percentages and compute contributions
    let total_score: f32 = power_stats.iter().map(|(s, _)| s.power_score).sum();
    let mut final_stats = Vec::new();
    for (mut stat, components) in power_stats {
        if total_score > 0.0 {
            stat.power_pct = (stat.power_score / total_score) * 100.0;
            if stat.power_score > 0.0 {
                stat.cpu_contribution = components.cpu / stat.power_score;
                stat.gpu_contribution = components.gpu / stat.power_score;
                stat.display_contribution = components.display / stat.power_score;
                stat.disk_contribution = components.disk / stat.power_score;
                stat.net_contribution = components.net / stat.power_score;
                stat.mem_contribution = components.mem / stat.power_score;
            }
        }
        final_stats.push(stat);
    }

    // Sort by power_pct descending
    final_stats.sort_by(|a, b| {
        b.power_pct
            .partial_cmp(&a.power_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    final_stats
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_power_stats_equal_cpu() {
        let mut app_metrics = HashMap::new();

        app_metrics.insert(
            "app1".to_string(),
            AppMetrics {
                cpu_pcts: vec![50.0],
                gpu_pcts: vec![],
                focus_ms: 0,
                disk_bytes: 0,
                net_bytes: 0,
                mem_rss: vec![],
            },
        );

        app_metrics.insert(
            "app2".to_string(),
            AppMetrics {
                cpu_pcts: vec![50.0],
                gpu_pcts: vec![],
                focus_ms: 0,
                disk_bytes: 0,
                net_bytes: 0,
                mem_rss: vec![],
            },
        );

        let stats = compute_power_stats(app_metrics, 1000);
        assert_eq!(stats.len(), 2);

        // Total power_pct should sum to ~100
        let total_pct: f32 = stats.iter().map(|s| s.power_pct).sum();
        assert!((total_pct - 100.0).abs() < 0.1);

        // Equal CPU usage should give ~50% each
        assert!((stats[0].power_pct - 50.0).abs() < 1.0);
        assert!((stats[1].power_pct - 50.0).abs() < 1.0);
    }

    #[test]
    fn test_compute_power_stats_contributions() {
        let mut app_metrics = HashMap::new();

        app_metrics.insert(
            "game".to_string(),
            AppMetrics {
                cpu_pcts: vec![40.0],
                gpu_pcts: vec![60.0],
                focus_ms: 0,
                disk_bytes: 0,
                net_bytes: 0,
                mem_rss: vec![],
            },
        );

        let stats = compute_power_stats(app_metrics, 1000);
        assert_eq!(stats.len(), 1);

        let game = &stats[0];
        // CPU score = 0.40 * 40 = 16
        // GPU score = 0.20 * 60 = 12
        // total = 28, so contributions are: cpu = 16/28, gpu = 12/28
        const EPSILON: f32 = 0.01;
        assert!((game.cpu_contribution - (16.0 / 28.0)).abs() < EPSILON);
        assert!((game.gpu_contribution - (12.0 / 28.0)).abs() < EPSILON);
        assert!(game.display_contribution < EPSILON);
    }

    #[test]
    fn test_compute_power_stats_empty() {
        let app_metrics = HashMap::new();
        let stats = compute_power_stats(app_metrics, 1000);
        assert_eq!(stats.len(), 0);
    }

    #[test]
    fn test_compute_power_stats_zero_window() {
        let mut app_metrics = HashMap::new();
        app_metrics.insert(
            "app".to_string(),
            AppMetrics {
                cpu_pcts: vec![50.0],
                gpu_pcts: vec![],
                focus_ms: 0,
                disk_bytes: 0,
                net_bytes: 0,
                mem_rss: vec![],
            },
        );

        let stats = compute_power_stats(app_metrics, 0);
        assert_eq!(stats.len(), 0);
    }
}
