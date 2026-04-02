//! Build script for eBPF kernel program.
//!
//! eBPF is Linux-specific. This script ensures we only build on Linux.

fn main() {
    // Ensure we only compile on Linux
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!();
        eprintln!("ERROR: heimwatch-ebpf requires Linux to build.");
        eprintln!("eBPF is a Linux kernel feature and cannot run on macOS or Windows.");
        eprintln!();
        eprintln!("If you are building for a non-Linux platform:");
        eprintln!("- The eBPF crates are excluded from the workspace (see root Cargo.toml)");
        eprintln!("- Build the main workspace: cargo build -p heimwatch");
        eprintln!();
        panic!("eBPF build only supported on Linux");
    }

    // Linux build: currently a stub
    // TODO: Implement proper eBPF cross-compilation when needed
    println!("cargo:warning=heimwatch-ebpf is currently a stub; full eBPF build TBD");
}
