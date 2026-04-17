fn main() {
    // Only build eBPF on Linux targets
    // macOS and Windows don't use eBPF for network monitoring
    #[cfg(target_os = "linux")]
    build_ebpf();
}

#[cfg(target_os = "linux")]
fn build_ebpf() {
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    let out_path = std::path::PathBuf::from(&out_dir);
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");

    println!("cargo:rerun-if-changed=../../bpf/heimwatch-ebpf/src/");
    println!("cargo:rerun-if-changed=../../bpf/heimwatch-ebpf-common/src/");

    // Use cargo directly to build eBPF (nightly with BPF target).
    // We build to a temporary directory and then move the artifact.
    let temp_target = out_path.join("ebpf_target");
    let binary_path = temp_target.join("bpfel-unknown-none/release/heimwatch-ebpf");

    // The manifest path is relative to the workspace root
    // manifest_dir is /path/to/heimwatch/crates/heimwatch-collector
    // We want /path/to/heimwatch/bpf/heimwatch-ebpf/Cargo.toml
    let crates_dir = std::path::PathBuf::from(&manifest_dir)
        .parent()
        .unwrap()
        .to_path_buf();
    let workspace_root = crates_dir.parent().unwrap().to_path_buf();
    let ebpf_manifest = workspace_root.join("bpf/heimwatch-ebpf/Cargo.toml");

    // CARGO_ENCODED_RUSTFLAGS uses \x1f as separator, not spaces
    let rustflags = "--cfg=bpf_target_arch=\"x86_64\"\x1f-Cdebuginfo=2\x1f-Clink-arg=--btf".to_string();

    let mut cmd = std::process::Command::new("rustup");
    cmd.args(["run", "nightly", "cargo", "build"])
        .args(["--manifest-path", ebpf_manifest.to_str().unwrap()])
        .args(["-Z", "build-std=core"])
        .args(["--target", "bpfel-unknown-none"])
        .args(["--target-dir", temp_target.to_str().unwrap()])
        .args(["--release", "--bins"])
        .env("CARGO_ENCODED_RUSTFLAGS", rustflags)
        .env_remove("RUSTC")
        .env_remove("RUSTC_WORKSPACE_WRAPPER");

    let status = cmd
        .status()
        .expect("Failed to execute cargo for eBPF build");
    if !status.success() {
        eprintln!("eBPF build failed");
        eprintln!("Note: eBPF compilation requires:");
        eprintln!("  1. rustup toolchain install nightly --component rust-src");
        eprintln!("  2. cargo install bpf-linker");
        panic!("Failed to build eBPF programs");
    }

    // Move the compiled eBPF binary to OUT_DIR
    if binary_path.exists() {
        let dst_path = out_path.join("heimwatch-ebpf");
        // Remove existing file/directory if it exists
        let _ = std::fs::remove_dir_all(&dst_path);
        let _ = std::fs::remove_file(&dst_path);

        // Copy the binary
        if let Err(e) = std::fs::copy(&binary_path, &dst_path) {
            eprintln!("Warning: Failed to copy eBPF binary: {}", e);
            panic!("Failed to finalize eBPF build");
        }
    } else {
        eprintln!("eBPF binary not found at: {:?}", binary_path);
        panic!("eBPF compilation did not produce the expected binary");
    }
}
