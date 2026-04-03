#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::path::Path;

/// Retrieves the process/app name from a given PID.
///
/// On Linux, reads `/proc/[PID]/cmdline` to get the full command line,
/// then extracts just the executable name from the path.
///
/// # Arguments
/// * `pid` - The process ID to look up
///
/// # Returns
/// * `Ok(name)` - The executable name (e.g., "zen" from "/app/zen/zen")
/// * `Err` - If the PID cannot be found or the file cannot be read
///
/// # Example
/// ```ignore
/// let name = get_process_name(1234)?;
/// assert_eq!(name, "zen");
/// ```
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub fn get_process_name(pid: u32) -> Result<String, Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    {
        get_process_name_linux(pid)
    }
    #[cfg(target_os = "macos")]
    {
        get_process_name_macos(pid)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn get_process_name(_pid: u32) -> Result<String, Box<dyn std::error::Error>> {
    Err("Process name lookup not implemented for this platform".into())
}

#[cfg(target_os = "linux")]
fn get_process_name_linux(pid: u32) -> Result<String, Box<dyn std::error::Error>> {
    use std::fs;

    // Try /proc/[PID]/cmdline first (more reliable for full paths)
    let cmdline_path = format!("/proc/{}/cmdline", pid);
    if let Ok(cmdline) = fs::read_to_string(&cmdline_path) {
        // cmdline uses null bytes as separators; take the first argument
        if let Some(exe_path) = cmdline.split('\0').next()
            && !exe_path.is_empty()
            && let Some(name) = extract_process_name(exe_path)
        {
            return Ok(name);
        }
    }

    // Fallback to /proc/[PID]/comm (limited to 15 chars but always available)
    let comm_path = format!("/proc/{}/comm", pid);
    let comm = fs::read_to_string(&comm_path)?;
    Ok(comm.trim().to_string())
}

#[cfg(target_os = "macos")]
fn get_process_name_macos(pid: u32) -> Result<String, Box<dyn std::error::Error>> {
    use std::process::Command;

    let output = Command::new("ps")
        .arg("-p")
        .arg(pid.to_string())
        .arg("-o")
        .arg("comm=")
        .output()?;

    if output.status.success() {
        let name = String::from_utf8(output.stdout)?;
        let trimmed = name.trim();
        if let Some(process_name) = extract_process_name(trimmed) {
            return Ok(process_name);
        } else {
            return Ok(trimmed.to_string());
        }
    } else {
        Err(format!("Process {} not found", pid).into())
    }
}

fn extract_process_name(path: &str) -> Option<String> {
    if let Some(base) = Path::new(path).file_name()
        && let Some(base_str) = base.to_str()
    {
        return Some(base_str.to_string());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "linux")]
    fn test_get_current_process_name() {
        // Get the current process's name
        let pid = std::process::id();
        let result = get_process_name(pid);

        // Should succeed for current process
        assert!(result.is_ok());

        let name = result.unwrap();
        // The current process should be some kind of test runner
        assert!(!name.is_empty());
        println!("Current process name: {}", name);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn test_init_process() {
        // PID 1 is always init
        let result = get_process_name(1);

        assert!(result.is_ok());
        let name = result.unwrap();
        // init could be 'init', 'systemd', or other init systems
        assert!(!name.is_empty());
        println!("PID 1 process name: {}", name);
    }

    #[test]
    fn test_invalid_pid() {
        // Try a very high PID that shouldn't exist
        let result = get_process_name(999999);
        assert!(result.is_err());
    }
}
