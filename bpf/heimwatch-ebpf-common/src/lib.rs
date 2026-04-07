#![no_std]

/// Per-PID network byte counters stored in the BPF HashMap.
/// Must be repr(C) so both kernel and user-space have identical layout.
/// repr(C) is mandatory for identical kernel/user-space memory layout. Rust's default repr(Rust) makes no ABI guarantees.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PidNetStats {
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    /// Process name captured at first packet, from the kernel task_struct comm field.
    /// Null-terminated, max 15 chars + null (TASK_COMM_LEN = 16).
    /// Captured in kernel-space so it's valid even after the process exits.
    pub comm: [u8; 16],
}
