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

/// Per-process CPU time accumulator stored in the BPF HashMap.
/// Map is keyed by process name (comm field), not PID, to avoid PID reuse conflicts.
/// Must be repr(C) for kernel/user-space ABI compatibility.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PidCpuStats {
    /// Cumulative nanoseconds on-CPU, accumulated across all sched_switch events.
    pub cpu_time_ns: u64,
    /// Timestamp (from bpf_ktime_get_ns) when this process was last scheduled on-CPU.
    /// Non-zero when the process is currently on-CPU; zero when off-CPU.
    pub last_sched_in_ns: u64,
}

/// Per-process disk I/O byte counters stored in the BPF HashMap.
/// Map is keyed by process name (comm field), not PID, to avoid PID reuse conflicts.
/// Must be repr(C) for kernel/user-space ABI compatibility.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PidDiskStats {
    /// Cumulative bytes read from block devices via block_rq_issue.
    pub read_bytes: u64,
    /// Cumulative bytes written to block devices via block_rq_issue.
    pub write_bytes: u64,
}
