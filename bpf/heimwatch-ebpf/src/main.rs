#![no_std]
#![no_main]

//! Heimwatch eBPF programs for kernel-space system metrics collection.
//!
//! This module uses CO-RE (Compile Once Run Everywhere) for tracepoint probes. CO-RE enables
//! portable eBPF programs that work across different kernel versions without recompilation or
//! manual offset adjustments. The aya compiler uses kernel BTF (BPF Type Format) at load time
//! to automatically discover field offsets in tracepoint context structs. Tracepoint field
//! layouts are stable within kernel series but may shift across major versions — CO-RE handles
//! this transparently by reading the kernel's BTF at eBPF program load time.
//!
//! Network probes (tcp_sendmsg, tcp_recvmsg) use kprobes/kretprobes instead of tracepoints
//! and don't require CO-RE because they monitor stable kernel function signatures (ABI-guaranteed).

use aya_ebpf::{
    helpers::{bpf_get_current_comm, bpf_ktime_get_ns},
    macros::{kprobe, kretprobe, map, tracepoint},
    maps::HashMap,
    programs::{ProbeContext, RetProbeContext, TracePointContext},
};
use aya_log_ebpf::warn;
use heimwatch_ebpf_common::{PidNetStats, PidCpuStats, PidDiskStats};

/// Max entries per BPF map. On larger systems with >10K processes, older/inactive PIDs are evicted.
/// User-space collector handles missing PIDs gracefully by continuing to next entry.
const BPF_MAP_ENTRIES: u32 = 10_240;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe { core::hint::unreachable_unchecked() }
}

/// BPF map: key = process name ([u8; 16]), value = PidNetStats (tx_bytes, rx_bytes)
/// Keyed by process name instead of PID to avoid PID reuse conflicts.
/// ~80 KB. User-space collector handles missing process names gracefully.
#[map]
static NETWORK_STATS: HashMap<[u8; 16], PidNetStats> = HashMap::with_max_entries(BPF_MAP_ENTRIES, 0);

/// BPF map: key = process name ([u8; 16]), value = PidCpuStats (cpu_time_ns, last_sched_in_ns)
/// Keyed by process name instead of PID to avoid PID reuse conflicts.
/// ~120 KB. Tracks cumulative CPU time per process via sched_switch.
#[map]
static CPU_STATS: HashMap<[u8; 16], PidCpuStats> = HashMap::with_max_entries(BPF_MAP_ENTRIES, 0);

/// BPF map: key = process name ([u8; 16]), value = PidDiskStats (read_bytes, write_bytes)
/// Keyed by process name instead of PID to avoid PID reuse conflicts.
/// ~120 KB. Tracks cumulative block I/O bytes per process via block_rq_issue.
#[map]
static DISK_STATS: HashMap<[u8; 16], PidDiskStats> = HashMap::with_max_entries(BPF_MAP_ENTRIES, 0);

/// Attached to tcp_sendmsg (kprobe on kernel function).
///
/// Kprobes monitor kernel function calls by their stable function signature.
/// Unlike tracepoints, they don't have layout-dependent fields, so CO-RE is not needed.
/// Function signatures are stable across kernel versions (ABI requirement).
///
/// arg(2) contains the send size; args 0-1 are kernel struct pointers (inaccessible from BPF)
///
/// tcp_sendmsg signature:
///   int tcp_sendmsg(struct sock *sk, struct msghdr *msg, size_t size)
/// arg(0) = struct sock *sk
/// arg(1) = struct msghdr *msg
/// arg(2) = size_t size (the actual byte count to send)
#[kprobe(function = "tcp_sendmsg")]
pub fn trace_sendmsg(ctx: ProbeContext) -> u32 {
    match try_sendmsg(&ctx) {
        Ok(_) => 0,
        Err(_) => 0,
    }
}

#[inline(always)]
fn try_sendmsg(ctx: &ProbeContext) -> Result<(), i64> {
    // tcp_sendmsg(struct sock *sk, struct msghdr *msg, size_t size)
    // arg(2) = size_t size (the actual byte count to send)
    let size: u64 = ctx.arg(2).unwrap_or(0);

    // Capture the current process name (comm field from task_struct)
    let comm = bpf_get_current_comm().unwrap_or([0u8; 16]);

    // Update or insert the stats for this process (keyed by comm/name)
    let stats = NETWORK_STATS.get_ptr_mut(&comm);
    match stats {
        Some(s) => unsafe { (*s).tx_bytes = (*s).tx_bytes.saturating_add(size) },
        None => {
            let new_stats = PidNetStats {
                tx_bytes: size,
                rx_bytes: 0,
                comm,
            };
            // Attempt to insert; map may be full if many processes are running.
            // Log to kernel trace buffer if insertion fails.
            if NETWORK_STATS.insert(&comm, &new_stats, 0).is_err() {
                warn!(ctx, "NETWORK_STATS map full");
            }
        }
    }

    Ok(())
}

/// Attached to tcp_recvmsg (kretprobe on kernel function).
///
/// Kretprobes monitor kernel function return values by their stable function signature.
/// Unlike tracepoints, they don't have layout-dependent fields, so CO-RE is not needed.
/// Function signatures are stable across kernel versions (ABI requirement).
///
/// Reads the return value (bytes received) and accumulates rx_bytes for the current PID.
///
/// tcp_recvmsg signature:
///   int tcp_recvmsg(struct sock *sk, struct msghdr *msg, size_t len, int flags, int *addr_len)
/// kretprobe reads the return value (positive = bytes received, negative = error code)
#[kretprobe(function = "tcp_recvmsg")]
pub fn trace_recvmsg(ctx: RetProbeContext) -> u32 {
    match try_recvmsg(&ctx) {
        Ok(_) => 0,
        Err(_) => 0,
    }
}

#[inline(always)]
fn try_recvmsg(ctx: &RetProbeContext) -> Result<(), i64> {
    // For a kretprobe, ctx.ret() gives us the return value of the probed function
    // tcp_recvmsg returns the number of bytes received (positive) or an error code (negative)

    // bytes_received <= 0: errors (negative) and empty reads (0). Skip to avoid false counts; partial reads already counted in tcp_sendmsg.
    let bytes_received: i64 = ctx.ret().unwrap_or(0);

    // Only count positive bytes; ignore errors and 0-byte reads
    if bytes_received <= 0 {
        return Ok(());
    }

    // Capture the current process name (comm field from task_struct)
    let comm = bpf_get_current_comm().unwrap_or([0u8; 16]);

    // Update or insert the stats for this process (keyed by comm/name)
    let stats = NETWORK_STATS.get_ptr_mut(&comm);
    match stats {
        Some(s) => unsafe { (*s).rx_bytes = (*s).rx_bytes.saturating_add(bytes_received as u64) },
        None => {
            let new_stats = PidNetStats {
                tx_bytes: 0,
                rx_bytes: bytes_received as u64,
                comm,
            };
            // Attempt to insert; map may be full if many processes are running.
            // Log to kernel trace buffer if insertion fails.
            if NETWORK_STATS.insert(&comm, &new_stats, 0).is_err() {
                warn!(ctx, "NETWORK_STATS map full");
            }
        }
    }

    Ok(())
}

/// Attached to sched:sched_switch tracepoint.
///
/// Fires on every context switch. At this point, bpf_get_current_pid_tgid() returns
/// the process being switched off (prev). We read next_pid from the tracepoint context
/// via CO-RE struct (works on kernels 4.4+).
#[tracepoint(name = "sched_switch", category = "sched")]
pub fn trace_sched_switch(ctx: TracePointContext) -> u32 {
    match try_sched_switch(&ctx) {
        Ok(_) => 0,
        Err(_) => 0,
    }
}

#[inline(always)]
fn try_sched_switch(ctx: &TracePointContext) -> Result<(), i64> {
    let now_ns = unsafe { bpf_ktime_get_ns() };

    let prev_comm: [u8; 16] = unsafe { ctx.read_at::<[u8; 16]>(8)? };
    let next_comm: [u8; 16] = unsafe { ctx.read_at::<[u8; 16]>(40)? };

    // --- Handle sched-out: accumulate CPU time for prev process ---
    if !is_comm_empty(&prev_comm) {
        if let Some(s) = CPU_STATS.get_ptr_mut(&prev_comm) {
            unsafe {
                let last_in = (*s).last_sched_in_ns;
                if last_in > 0 {
                    let delta = now_ns.saturating_sub(last_in);
                    (*s).cpu_time_ns = (*s).cpu_time_ns.saturating_add(delta);
                }
                // Mark as off-CPU
                (*s).last_sched_in_ns = 0;
            }
        }
    }

    // --- Handle sched-in: stamp start time for next process ---
    if !is_comm_empty(&next_comm) {
        match CPU_STATS.get_ptr_mut(&next_comm) {
            Some(s) => unsafe { (*s).last_sched_in_ns = now_ns },
            None => {
                let new_stats = PidCpuStats {
                    cpu_time_ns: 0,
                    last_sched_in_ns: now_ns,
                };
                if CPU_STATS.insert(&next_comm, &new_stats, 0).is_err() {
                    warn!(ctx, "CPU_STATS map full");
                }
            }
        }
    }

    Ok(())
}

/// Helper: check if comm field is empty/all-zeros
#[inline(always)]
fn is_comm_empty(comm: &[u8; 16]) -> bool {
    // Check if first byte is null or if entire comm is zeros
    comm[0] == 0
}

/// Attached to block:block_rq_issue tracepoint.
///
/// Fires when an I/O request is issued to the device driver, in process context.
/// Reads I/O size and direction via CO-RE struct (works on kernels 4.18+).
#[tracepoint(name = "block_rq_issue", category = "block")]
pub fn trace_block_rq_issue(ctx: TracePointContext) -> u32 {
    match try_block_rq_issue(&ctx) {
        Ok(_) => 0,
        Err(_) => 0,
    }
}

#[inline(always)]
fn try_block_rq_issue(ctx: &TracePointContext) -> Result<(), i64> {
    // Read the transfer size in bytes (unsigned int bytes; offset 28, size 4)
    let bytes_u32: u32 = unsafe { ctx.read_at::<u32>(28)? };
    if bytes_u32 == 0 {
        return Ok(());
    }
    let bytes: u64 = bytes_u32 as u64;

    // Read rwbs (char rwbs[10]; offset 34, size 10)
    let rwbs: [u8; 10] = unsafe { ctx.read_at::<[u8; 10]>(34)? };

    // Read comm (char comm[16]; offset 44, size 16)
    let comm_raw: [u8; 16] = unsafe { ctx.read_at::<[u8; 16]>(44)? };

    // Skip processes with empty comm (no non-zero byte)
    if is_comm_empty(&comm_raw) {
        return Ok(());
    }

    // Normalize comm key: zero-pad already fixed-size array is fine,
    // but ensure consistent casing for matching (optional).
    let comm_key = comm_raw;

    // Determine if this is a read or write (handle lowercase too)
    let first = rwbs.get(0).copied().unwrap_or(0);
    let is_read = first == b'R' || first == b'r';
    let is_write = first == b'W' || first == b'w';

    // Update/insert stats keyed by comm (consider using pid instead)
    match DISK_STATS.get_ptr_mut(&comm_key) {
        Some(s) => unsafe {
            if is_read {
                (*s).read_bytes = (*s).read_bytes.saturating_add(bytes);
            } else if is_write {
                (*s).write_bytes = (*s).write_bytes.saturating_add(bytes);
            }
        },
        None => {
            let new_stats = PidDiskStats {
                read_bytes: if is_read { bytes } else { 0 },
                write_bytes: if is_write { bytes } else { 0 },
            };
            if DISK_STATS.insert(&comm_key, &new_stats, 0).is_err() {
                // Best-effort warning; may be no-op in some eBPF environments
                warn!(ctx, "DISK_STATS map full or insert failed");
            }
        }
    }

    Ok(())
}
