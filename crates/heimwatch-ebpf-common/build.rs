//! Build script for eBPF common types.
//!
//! These types are specific to Linux eBPF and kernel-space code.

fn main() {
    // Ensure we only compile on Linux
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!();
        eprintln!("ERROR: heimwatch-ebpf-common requires Linux to build.");
        eprintln!("These types are specific to Linux eBPF kernel-space code.");
        eprintln!();
        eprintln!("If you are building for a non-Linux platform:");
        eprintln!("- The eBPF crates are excluded from the workspace (see root Cargo.toml)");
        eprintln!("- Build the main workspace: cargo build -p heimwatch");
        eprintln!();
        panic!("eBPF common types only supported on Linux");
    }
}
