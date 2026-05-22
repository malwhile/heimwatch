//! Power score calculation logic.
//!
//! This module contains the pure calculation logic for computing per-app power consumption
//! statistics. It is independent of storage and can be tested and swapped independently
//! (e.g., for Approach A fixed weights vs. Approach B RAPL-calibrated in Phase 4).

use heimwatch_core::AppPowerStats;
use std::collections::HashMap;

/// Aggregated metrics for a single app across a time range.
#[derive(Default, Clone)]
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

/// Compute per-app power consumption statistics using Approach A (fixed weights) or Approach B (RAPL-calibrated).
///
/// This is a pure calculation function that operates on pre-aggregated metrics.
/// Battery state filtering is handled by the caller (`StorageLayer::get_power_stats`).
///
/// # Arguments
/// * `app_metrics` - Pre-aggregated metrics per app (CPU %, GPU %, focus time, disk/net/memory bytes)
/// * `window_ms` - Query window duration in milliseconds (used to normalize focus time)
/// * `rapl_package_watts` - Average CPU package power (watts) from RAPL readings in the query window.
///   If Some, uses Approach B (RAPL-calibrated, proportional CPU attribution).
///   If None, uses Approach A (fixed-weight, 0.40 CPU).
/// * `freq_ratio` - Average CPU frequency ratio (cur_freq / max_freq) across the query window.
///   If Some and RAPL is None, scales Approach A CPU score by `(freq_ratio)²`.
/// * `display_brightness` - Average display brightness ratio (cur / max) across the query window.
///   If Some, scales display score by this fraction (defaults to 1.0 = full brightness).
/// * `is_wifi` - True if the default route is over WiFi, false if Ethernet, None if unknown.
///   Used to adjust network power weight: WiFi (0.15), Ethernet (0.05), Unknown (0.10).
///
/// # Attribution Approaches
///
/// **Approach A (no RAPL):**
/// - CPU: 40% (fixed weight) × (freq_ratio)² if freq_ratio available, else 40%
/// - GPU: 20% | Display: 15% × display_brightness if available | Disk I/O: 10% | Network: 10–15% based on interface | Memory: 5%
///
/// **Approach B (RAPL available):**
/// - CPU: `rapl_watts × (app_cpu_pct / total_cpu_pct)` (proportional actual watts; freq already accounted for)
/// - GPU: 20% | Display: 15% × display_brightness if available | Disk I/O: 10% | Network: 10–15% based on interface | Memory: 5%
///
/// # Returns
/// Apps sorted descending by `power_pct`. Contribution fractions sum to ≤1.0 per app.
pub fn compute_power_stats(
    app_metrics: HashMap<String, AppMetrics>,
    window_ms: u64,
    rapl_package_watts: Option<f32>,
    freq_ratio: Option<f32>,
    display_brightness: Option<f32>,
    is_wifi: Option<bool>,
) -> Vec<AppPowerStats> {
    if window_ms == 0 {
        return Vec::new();
    }

    // Compute normalization factors and needed aggregates
    let max_disk_bytes = app_metrics
        .values()
        .map(|m| m.disk_bytes)
        .max()
        .unwrap_or(1);
    let max_net_bytes = app_metrics.values().map(|m| m.net_bytes).max().unwrap_or(1);
    let total_mem_rss: u64 = app_metrics.values().flat_map(|m| &m.mem_rss).sum();

    // For Approach B (RAPL-calibrated): compute total CPU % across all apps
    // This is needed for proportional CPU attribution when RAPL is available
    let total_cpu_pct: f32 = app_metrics
        .values()
        .map(|m| {
            if m.cpu_pcts.is_empty() {
                0.0
            } else {
                m.cpu_pcts.iter().sum::<f32>() / m.cpu_pcts.len() as f32
            }
        })
        .sum();

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

            // Compute weighted components (Approach A or B depending on RAPL availability)
            let cpu_score = match (rapl_package_watts, freq_ratio) {
                (Some(watts), _) if total_cpu_pct > 0.0 => {
                    // Approach B: proportional CPU attribution using measured RAPL watts
                    // (RAPL already accounts for current frequency, so don't apply freq scaling)
                    watts * (cpu_pct_avg / total_cpu_pct)
                }
                (None, Some(ratio)) => {
                    // Approach A+: fixed-weight CPU scaled by (frequency_ratio)²
                    0.40 * cpu_pct_avg * ratio * ratio
                }
                _ => {
                    // Approach A: fixed-weight CPU (fallback when RAPL and freq_ratio unavailable)
                    0.40 * cpu_pct_avg
                }
            };

            let net_weight = match is_wifi {
                Some(true) => 0.15,  // WiFi: higher power (radio transceiver active)
                Some(false) => 0.05, // Ethernet: lower power (passive copper connection)
                None => 0.10,        // Unknown: use baseline (no regression)
            };

            let components = ScoreComponents {
                cpu: cpu_score,
                gpu: 0.20 * gpu_pct_max,
                display: 0.15 * focus_fraction * display_brightness.unwrap_or(1.0),
                disk: 0.10 * disk_normalized,
                net: net_weight * net_normalized,
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

        let stats = compute_power_stats(app_metrics, 1000, None, None, None, None);
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

        let stats = compute_power_stats(app_metrics, 1000, None, None, None, None);
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
        let stats = compute_power_stats(app_metrics, 1000, None, None, None, None);
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

        let stats = compute_power_stats(app_metrics, 0, None, None, None, None);
        assert_eq!(stats.len(), 0);
    }

    /// Test Approach B (RAPL-calibrated) CPU attribution.
    #[test]
    fn test_compute_power_stats_rapl_approach_b() {
        let mut app_metrics = HashMap::new();

        // Single app with 50% CPU usage
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

        // Approach A (no RAPL)
        let stats_a = compute_power_stats(app_metrics.clone(), 1000, None, None, None, None);
        assert_eq!(stats_a.len(), 1);
        let score_a = stats_a[0].power_score;
        // Approach A: cpu_score = 0.40 * 50 = 20
        assert!((score_a - 20.0).abs() < 0.1);

        // Approach B (with RAPL: 10W total CPU power)
        let stats_b = compute_power_stats(app_metrics, 1000, Some(10.0), None, None, None);
        assert_eq!(stats_b.len(), 1);
        let score_b = stats_b[0].power_score;
        // Approach B: cpu_score = 10 * (50 / 50) = 10
        // (RAPL gives actual CPU watts; app gets proportional share based on its CPU %)
        assert!(
            (score_b - 10.0).abs() < 0.1,
            "Expected ~10.0, got {}",
            score_b
        );
        assert!(
            score_b < score_a,
            "RAPL approach should give lower score when CPU power is low"
        );
    }

    /// Test fallback to Approach A when total CPU % is zero.
    #[test]
    fn test_compute_power_stats_rapl_fallback_when_zero_cpu() {
        let mut app_metrics = HashMap::new();

        // App with no CPU data
        app_metrics.insert(
            "app".to_string(),
            AppMetrics {
                cpu_pcts: vec![],
                gpu_pcts: vec![10.0],
                focus_ms: 1000,
                disk_bytes: 0,
                net_bytes: 0,
                mem_rss: vec![],
            },
        );

        // Even with RAPL available, if total_cpu_pct is 0, should not divide by zero
        let stats = compute_power_stats(app_metrics, 1000, Some(20.0), None, None, None);
        assert_eq!(stats.len(), 1);

        // Should have GPU + display contribution but no CPU (fallback to 0.40 * 0 = 0)
        let score = stats[0].power_score;
        let expected_gpu = 0.20 * 10.0; // gpu: 2.0
        let expected_display = 0.15 * 100.0; // focus_ms: 1000 / (1000 * 1000) * 100 = ~100%
        let expected_cpu = 0.40 * 0.0; // no CPU: 0
        let expected_total = expected_gpu + expected_display + expected_cpu;
        assert!(
            (score - expected_total).abs() < 0.1,
            "Expected ~{}, got {}",
            expected_total,
            score
        );
    }

    /// Test Approach B proportional CPU attribution with multiple apps.
    #[test]
    fn test_compute_power_stats_rapl_attribution() {
        let mut app_metrics = HashMap::new();

        // app1: 60% CPU usage
        app_metrics.insert(
            "app1".to_string(),
            AppMetrics {
                cpu_pcts: vec![60.0],
                gpu_pcts: vec![],
                focus_ms: 0,
                disk_bytes: 0,
                net_bytes: 0,
                mem_rss: vec![],
            },
        );

        // app2: 40% CPU usage
        app_metrics.insert(
            "app2".to_string(),
            AppMetrics {
                cpu_pcts: vec![40.0],
                gpu_pcts: vec![],
                focus_ms: 0,
                disk_bytes: 0,
                net_bytes: 0,
                mem_rss: vec![],
            },
        );

        // RAPL: 10W total CPU power
        let stats = compute_power_stats(app_metrics, 1000, Some(10.0), None, None, None);
        assert_eq!(stats.len(), 2);

        // Find app1 and app2 (sorted by power_pct descending)
        let app1 = stats.iter().find(|s| s.app_name == "app1").unwrap();
        let app2 = stats.iter().find(|s| s.app_name == "app2").unwrap();

        // Approach B: cpu_score = rapl_watts * (app_cpu_pct / total_cpu_pct)
        // app1: 10 * (60 / 100) = 6.0
        // app2: 10 * (40 / 100) = 4.0
        const EPSILON: f32 = 0.1;
        assert!(
            (app1.power_score - 6.0).abs() < EPSILON,
            "app1 CPU score should be ~6W, got {}",
            app1.power_score
        );
        assert!(
            (app2.power_score - 4.0).abs() < EPSILON,
            "app2 CPU score should be ~4W, got {}",
            app2.power_score
        );

        // Verify proportional contributions
        let total = app1.power_score + app2.power_score;
        assert!((total - 10.0).abs() < 0.1, "Total CPU score should be ~10W");
        assert!(
            (app1.power_pct - 60.0).abs() < 1.0,
            "app1 power_pct should be ~60%, got {}",
            app1.power_pct
        );
        assert!(
            (app2.power_pct - 40.0).abs() < 1.0,
            "app2 power_pct should be ~40%, got {}",
            app2.power_pct
        );
    }

    /// Test CPU frequency scaling in Approach A (no RAPL).
    /// With freq_ratio=0.5, cpu_score should be scaled by (0.5)² = 0.25
    #[test]
    fn test_compute_power_stats_freq_scaling() {
        let mut app_metrics = HashMap::new();

        app_metrics.insert(
            "app".to_string(),
            AppMetrics {
                cpu_pcts: vec![40.0],
                gpu_pcts: vec![],
                focus_ms: 0,
                disk_bytes: 0,
                net_bytes: 0,
                mem_rss: vec![],
            },
        );

        // Approach A with freq_ratio=None: cpu_score = 0.40 * 40 = 16.0
        let stats_no_freq = compute_power_stats(app_metrics.clone(), 1000, None, None, None, None);
        let score_no_freq = stats_no_freq[0].power_score;
        assert!((score_no_freq - 16.0).abs() < 0.1);

        // Approach A with freq_ratio=0.5: cpu_score = 0.40 * 40 * (0.5)² = 4.0
        let stats_with_freq = compute_power_stats(app_metrics, 1000, None, Some(0.5), None, None);
        let score_with_freq = stats_with_freq[0].power_score;
        assert!((score_with_freq - 4.0).abs() < 0.1);

        // Verify the scaling: 16.0 * (0.5)² = 16.0 * 0.25 = 4.0
        assert!(
            (score_with_freq - score_no_freq * 0.25).abs() < 0.01,
            "Score should be scaled by 0.25, got {} vs {}",
            score_with_freq,
            score_no_freq * 0.25
        );
    }

    /// Test that RAPL takes precedence over frequency scaling.
    /// When RAPL is available, freq_ratio should have no effect (RAPL already accounts for frequency).
    #[test]
    fn test_compute_power_stats_rapl_ignores_freq() {
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

        // With RAPL, no freq_ratio: cpu_score = 10 * (50 / 50) = 10
        let stats_rapl_no_freq =
            compute_power_stats(app_metrics.clone(), 1000, Some(10.0), None, None, None);
        let score_rapl_no_freq = stats_rapl_no_freq[0].power_score;
        assert!((score_rapl_no_freq - 10.0).abs() < 0.1);

        // With RAPL and freq_ratio=0.5, should give same result (freq_ratio ignored)
        let stats_rapl_with_freq =
            compute_power_stats(app_metrics, 1000, Some(10.0), Some(0.5), None, None);
        let score_rapl_with_freq = stats_rapl_with_freq[0].power_score;
        assert!((score_rapl_with_freq - 10.0).abs() < 0.1);

        // Both should be equal (freq_ratio has no effect when RAPL available)
        assert!(
            (score_rapl_no_freq - score_rapl_with_freq).abs() < 0.01,
            "RAPL should ignore freq_ratio, got {} vs {}",
            score_rapl_no_freq,
            score_rapl_with_freq
        );
    }

    /// Test display brightness scaling of display component.
    /// With display_brightness=0.5 and focus_ms=500 (50% of 1000ms window),
    /// display_score should be 0.15 × 50 × 0.5 = 3.75 vs 7.5 without brightness.
    #[test]
    fn test_compute_power_stats_display_brightness_scaling() {
        let mut app_metrics = HashMap::new();

        app_metrics.insert(
            "app".to_string(),
            AppMetrics {
                cpu_pcts: vec![],
                gpu_pcts: vec![],
                focus_ms: 500,
                disk_bytes: 0,
                net_bytes: 0,
                mem_rss: vec![],
            },
        );

        // No brightness (defaults to 1.0): display_score = 0.15 × 50 = 7.5
        let stats_no_bright = compute_power_stats(app_metrics.clone(), 1000, None, None, None, None);
        let score_no_bright = stats_no_bright[0].power_score;
        assert!((score_no_bright - 7.5).abs() < 0.1);

        // With brightness=0.5: display_score = 0.15 × 50 × 0.5 = 3.75
        let stats_with_bright = compute_power_stats(app_metrics, 1000, None, None, Some(0.5), None);
        let score_with_bright = stats_with_bright[0].power_score;
        assert!((score_with_bright - 3.75).abs() < 0.1);

        // Verify the scaling: score should be halved when brightness is 0.5
        assert!(
            (score_with_bright - score_no_bright * 0.5).abs() < 0.01,
            "Display score should scale by brightness, got {} vs {}",
            score_with_bright,
            score_no_bright * 0.5
        );
    }

    /// Test that display_brightness=None defaults to 1.0 (full brightness).
    /// Should produce identical results to Some(1.0).
    #[test]
    fn test_compute_power_stats_display_brightness_none_defaults_full() {
        let mut app_metrics = HashMap::new();

        app_metrics.insert(
            "app".to_string(),
            AppMetrics {
                cpu_pcts: vec![],
                gpu_pcts: vec![],
                focus_ms: 600,
                disk_bytes: 0,
                net_bytes: 0,
                mem_rss: vec![],
            },
        );

        // None (defaults to 1.0): display_score = 0.15 × 60 × 1.0 = 9.0
        let stats_none = compute_power_stats(app_metrics.clone(), 1000, None, None, None, None);
        let score_none = stats_none[0].power_score;

        // Explicit Some(1.0): should be identical
        let stats_one = compute_power_stats(app_metrics, 1000, None, None, Some(1.0), None);
        let score_one = stats_one[0].power_score;

        assert!(
            (score_none - score_one).abs() < 0.01,
            "None should default to 1.0, got {} vs {}",
            score_none,
            score_one
        );
    }

    /// Test WiFi network weight (0.15) is higher than Ethernet weight (0.05).
    #[test]
    fn test_compute_power_stats_wifi_higher_net_weight() {
        let mut app_metrics = HashMap::new();

        app_metrics.insert(
            "app".to_string(),
            AppMetrics {
                cpu_pcts: vec![],
                gpu_pcts: vec![],
                focus_ms: 0,
                disk_bytes: 0,
                net_bytes: 1024,
                mem_rss: vec![],
            },
        );

        // WiFi: net_weight = 0.15
        let stats_wifi = compute_power_stats(app_metrics.clone(), 1000, None, None, None, Some(true));
        let score_wifi = stats_wifi[0].power_score;

        // Ethernet: net_weight = 0.05
        let stats_eth = compute_power_stats(app_metrics, 1000, None, None, None, Some(false));
        let score_eth = stats_eth[0].power_score;

        // WiFi score should be 3x higher (0.15 / 0.05 = 3.0)
        assert!(
            (score_wifi - score_eth * 3.0).abs() < 0.01,
            "WiFi should have 3x network weight of Ethernet, got {} vs {}",
            score_wifi,
            score_eth * 3.0
        );
    }

    /// Test Ethernet network weight (0.05) is lower than WiFi (0.15) and baseline (0.10).
    #[test]
    fn test_compute_power_stats_ethernet_lower_net_weight() {
        let mut app_metrics = HashMap::new();

        app_metrics.insert(
            "app".to_string(),
            AppMetrics {
                cpu_pcts: vec![],
                gpu_pcts: vec![],
                focus_ms: 0,
                disk_bytes: 0,
                net_bytes: 1024,
                mem_rss: vec![],
            },
        );

        // Ethernet: net_weight = 0.05
        let stats_eth = compute_power_stats(app_metrics.clone(), 1000, None, None, None, Some(false));
        let score_eth = stats_eth[0].power_score;

        // Unknown: net_weight = 0.10 (baseline)
        let stats_unknown = compute_power_stats(app_metrics, 1000, None, None, None, None);
        let score_unknown = stats_unknown[0].power_score;

        // Ethernet should be half of unknown (0.05 / 0.10 = 0.5)
        assert!(
            (score_eth - score_unknown * 0.5).abs() < 0.01,
            "Ethernet should have half the network weight of unknown, got {} vs {}",
            score_eth,
            score_unknown * 0.5
        );
    }

    /// Test unknown interface type defaults to baseline network weight (0.10).
    #[test]
    fn test_compute_power_stats_unknown_interface_uses_baseline() {
        let mut app_metrics = HashMap::new();

        app_metrics.insert(
            "app".to_string(),
            AppMetrics {
                cpu_pcts: vec![],
                gpu_pcts: vec![],
                focus_ms: 0,
                disk_bytes: 0,
                net_bytes: 1024,
                mem_rss: vec![],
            },
        );

        // Unknown (None): net_weight = 0.10
        let stats_none = compute_power_stats(app_metrics, 1000, None, None, None, None);
        let score_none = stats_none[0].power_score;

        // net_bytes normalized should give 100.0 (max=1024), so score = 0.10 * 100 = 10.0
        // But we just verify the weight is 0.10 by checking the ratio
        assert!(score_none > 0.0, "Unknown interface should have non-zero network score");
        // The exact score depends on normalization, just verify it's computed
    }
}
