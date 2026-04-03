#[cfg(target_os = "linux")]
use std::path::PathBuf;

fn main() {
    // Only build eBPF on Linux targets
    // macOS and Windows don't use eBPF for network monitoring
    #[cfg(target_os = "linux")]
    build_ebpf_stub();
}

#[cfg(target_os = "linux")]
fn build_ebpf_stub() {
    // TODO: Implement proper eBPF cross-compilation using custom target specs or aya-build
    // For now, create a minimal stub ELF to allow the rest of the code to compile

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let stub_path = PathBuf::from(&out_dir).join("heimwatch-ebpf");

    // Minimal ELF header (stub for development - actual BPF build needed later)
    // This is a valid ELF file but will fail at runtime without actual BPF bytecode
    let elf_stub = [
        0x7f, 0x45, 0x4c, 0x46, // ELF magic
        0x01, // 32-bit (stub)
        0x01, // little endian
        0x01, // ELF version
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // rest of e_ident
        0x00, 0x00, // e_type (ET_NONE)
        0x00, 0x00, // e_machine
        0x00, 0x00, 0x00, 0x00, // e_version
    ];

    std::fs::write(&stub_path, elf_stub).expect("Failed to write ELF stub");

    println!("cargo:rerun-if-changed=crates/heimwatch-ebpf/src/");
    println!("cargo:rerun-if-changed=crates/heimwatch-ebpf-common/src/");
}
