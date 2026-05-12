//! /proc/<pid>/fdinfo DRM scanner for per-process GPU usage tracking.
//!
//! Scans /proc filesystem to find DRM file descriptors and extract engine time / VRAM usage
//! per process. Supports AMD (engine-gfx ns-based), Intel/NVIDIA (cycle counter-based).

use std::collections::HashMap;
use std::fs;

/// Raw per-fd metrics parsed from one DRM fdinfo file.
struct DrmFdStats {
    pdev: String,
    engine_gfx_ns: Option<u64>,
    cycles_gfx: Option<u64>,
    total_cycles_gfx: Option<u64>,
    memory_vram_bytes: Option<u64>,
}

/// Aggregated per-process stats for one GPU.
pub struct ProcessGpuStats {
    pub pdev: String,
    /// AMD: cumulative ns. Intel/NVIDIA: cumulative cycles.
    pub engine_time: u64,
    /// Intel/NVIDIA only: drm-total-cycles-gfx from the window.
    pub total_cycles: u64,
    /// false = AMD (ns-based); true = Intel/NVIDIA (cycles-based).
    pub is_cycles_schema: bool,
    pub vram_bytes: u64,
}

/// Scan /proc for all processes with DRM file descriptors and return per-process GPU stats.
pub fn scan_proc_fdinfo(
    known_pdevs: &std::collections::HashSet<String>,
) -> Vec<(u32, String, ProcessGpuStats)> {
    let mut result = Vec::new();

    let proc_dir = match fs::read_dir("/proc") {
        Ok(d) => d,
        Err(_) => return result,
    };

    for entry in proc_dir {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Keep only all-digit entries (PIDs).
        if !name_str.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }

        let pid: u32 = match name_str.parse() {
            Ok(p) => p,
            Err(_) => continue,
        };

        // Read app name from /proc/<pid>/comm
        let app_name = match fs::read_to_string(format!("/proc/{}/comm", pid)) {
            Ok(content) => content.trim().to_string(),
            Err(_) => format!("pid:{}", pid),
        };

        // Read fdinfo directory
        let fdinfo_dir = format!("/proc/{}/fdinfo", pid);
        let fdinfo_entries = match fs::read_dir(&fdinfo_dir) {
            Ok(d) => d,
            Err(_) => continue, // Process may have exited
        };

        // Collect stats per (pid, pdev)
        let mut by_pdev: HashMap<String, (ProcessGpuStats, bool)> = HashMap::new();

        for fd_entry in fdinfo_entries {
            let fd_entry = match fd_entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            let fd_contents = match fs::read_to_string(fd_entry.path()) {
                Ok(c) => c,
                Err(_) => continue,
            };

            if let Some(stats) = parse_drm_fdinfo(&fd_contents) {
                if !known_pdevs.contains(&stats.pdev) {
                    continue;
                }

                let is_cycles = stats.cycles_gfx.is_some();
                let engine_time = stats.engine_gfx_ns.or(stats.cycles_gfx).unwrap_or_default();

                let entry = by_pdev.entry(stats.pdev.clone()).or_insert((
                    ProcessGpuStats {
                        pdev: stats.pdev.clone(),
                        engine_time: 0,
                        total_cycles: 0,
                        is_cycles_schema: is_cycles,
                        vram_bytes: 0,
                    },
                    false,
                ));

                entry.0.engine_time = entry.0.engine_time.saturating_add(engine_time);
                if let Some(tc) = stats.total_cycles_gfx {
                    // Keep the total_cycles value from any fd that has it.
                    entry.0.total_cycles = entry.0.total_cycles.max(tc);
                }
                if let Some(vram) = stats.memory_vram_bytes {
                    entry.0.vram_bytes = entry.0.vram_bytes.saturating_add(vram);
                }
                entry.1 = true; // Mark that we found at least one DRM fd.
            }
        }

        // Emit results for this pid
        for (_pdev, (stats, _)) in by_pdev {
            result.push((pid, app_name.clone(), stats));
        }
    }

    result
}

/// Parse a single fdinfo file and extract DRM metrics if present.
fn parse_drm_fdinfo(contents: &str) -> Option<DrmFdStats> {
    let mut pdev = String::new();
    let mut engine_gfx_ns = None;
    let mut cycles_gfx = None;
    let mut total_cycles_gfx = None;
    let mut memory_vram_bytes = None;

    for line in contents.lines() {
        if let Some(val) = line.strip_prefix("drm-pdev:\t") {
            pdev = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("drm-engine-gfx:\t") {
            if let Some(ns_str) = val.strip_suffix(" ns") {
                engine_gfx_ns = ns_str.trim().parse().ok();
            }
        } else if let Some(val) = line.strip_prefix("drm-cycles-gfx:\t") {
            cycles_gfx = val.trim().parse().ok();
        } else if let Some(val) = line.strip_prefix("drm-total-cycles-gfx:\t") {
            total_cycles_gfx = val.trim().parse().ok();
        } else if let Some(val) = line.strip_prefix("drm-memory-vram:\t") {
            memory_vram_bytes = parse_memory_size(val);
        }
    }

    // Gate: must have pdev
    if pdev.is_empty() {
        return None;
    }

    Some(DrmFdStats {
        pdev,
        engine_gfx_ns,
        cycles_gfx,
        total_cycles_gfx,
        memory_vram_bytes,
    })
}

/// Parse memory size with unit suffix (e.g., "512 KiB", "2 MiB", "1 GiB" or raw bytes).
fn parse_memory_size(s: &str) -> Option<u64> {
    let s = s.trim();

    // Check for unit suffix
    if let Some(num_str) = s.strip_suffix(" KiB") {
        num_str.trim().parse::<u64>().ok().map(|n| n * 1024)
    } else if let Some(num_str) = s.strip_suffix(" MiB") {
        num_str.trim().parse::<u64>().ok().map(|n| n * 1024 * 1024)
    } else if let Some(num_str) = s.strip_suffix(" GiB") {
        num_str
            .trim()
            .parse::<u64>()
            .ok()
            .map(|n| n * 1024 * 1024 * 1024)
    } else {
        // Try raw number (bytes)
        s.parse::<u64>().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_drm_fdinfo_amd() {
        let content = r#"pos:    0
flags:  02000002
mnt_id: 10
ino:    1234
drm-pdev:	0000:03:00.0
drm-engine-gfx:	5000000000 ns
drm-memory-vram:	512 MiB
"#;
        let stats = parse_drm_fdinfo(content).unwrap();
        assert_eq!(stats.pdev, "0000:03:00.0");
        assert_eq!(stats.engine_gfx_ns, Some(5000000000));
        assert_eq!(stats.memory_vram_bytes, Some(512 * 1024 * 1024));
        assert_eq!(stats.cycles_gfx, None);
    }

    #[test]
    fn test_parse_drm_fdinfo_intel() {
        let content = r#"pos:    0
flags:  02000002
mnt_id: 10
ino:    1234
drm-pdev:	0000:00:02.0
drm-cycles-gfx:	1000000
drm-total-cycles-gfx:	2000000
drm-memory-vram:	256 MiB
"#;
        let stats = parse_drm_fdinfo(content).unwrap();
        assert_eq!(stats.pdev, "0000:00:02.0");
        assert_eq!(stats.cycles_gfx, Some(1000000));
        assert_eq!(stats.total_cycles_gfx, Some(2000000));
        assert_eq!(stats.memory_vram_bytes, Some(256 * 1024 * 1024));
        assert_eq!(stats.engine_gfx_ns, None);
    }

    #[test]
    fn test_parse_drm_fdinfo_no_pdev() {
        let content = r#"pos:    0
flags:  02000002
mnt_id: 10
ino:    1234
"#;
        let stats = parse_drm_fdinfo(content);
        assert!(stats.is_none());
    }

    #[test]
    fn test_parse_memory_size_kib() {
        assert_eq!(parse_memory_size("512 KiB"), Some(512 * 1024));
    }

    #[test]
    fn test_parse_memory_size_mib() {
        assert_eq!(parse_memory_size("2 MiB"), Some(2 * 1024 * 1024));
    }

    #[test]
    fn test_parse_memory_size_gib() {
        assert_eq!(parse_memory_size("1 GiB"), Some(1024 * 1024 * 1024));
    }

    #[test]
    fn test_parse_memory_size_raw() {
        assert_eq!(parse_memory_size("1048576"), Some(1048576));
    }
}
