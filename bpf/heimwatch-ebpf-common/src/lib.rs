#![no_std]

/// Per-PID network byte counters stored in the BPF HashMap.
/// Must be repr(C) so both kernel and user-space have identical layout.
/// repr(C) is mandatory for identical kernel/user-space memory layout. Rust's default repr(Rust) makes no ABI guarantees.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PidNetStats {
    pub tx_bytes: u64,
    pub rx_bytes: u64,
}
