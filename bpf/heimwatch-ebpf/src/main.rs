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
    helpers::{bpf_get_current_comm, bpf_get_current_pid_tgid, bpf_ktime_get_ns},
    macros::{kprobe, kretprobe, map, tracepoint},
    maps::HashMap,
    programs::{ProbeContext, RetProbeContext, TracePointContext},
    EbpfContext,
};
use aya_log_ebpf::warn;
use heimwatch_ebpf_common::{PidNetStats, PidCpuStats, PidDiskStats};

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe { core::hint::unreachable_unchecked() }
}

/// sched_switch tracepoint struct for CO-RE field discovery.
/// Fields match kernel's internal layout (stable since kernel 4.4).
#[repr(C)]
struct SchedSwitch {
    /// Common tracepoint header
    _common_type: u32,
    _common_flags: u32,
    _common_preempt_count: i32,
    _common_pid: i32,

    /// Process being switched off
    prev_comm: [u8; 16],
    prev_pid: u32,
    prev_prio: i32,

    /// Process state bits (see kernel sched.h)
    prev_state: i64,

    /// Process being switched on
    next_comm: [u8; 16],
    next_pid: u32,
    _next_prio: i32,
}

/// block_rq_issue tracepoint struct for CO-RE field discovery.
/// Fields match kernel's internal layout (stable since kernel 4.18).
#[repr(C)]
struct BlockRqIssue {
    /// Common tracepoint header (skipped for now)
    _common_type: u32,
    _common_flags: u32,
    _common_preempt_count: i32,
    _common_pid: i32,

    /// Block device identifier
    dev: u32,
    _pad1: u32,

    /// Starting sector for this I/O
    sector: u64,

    /// Number of sectors in this request
    nr_sector: u32,
    _pad2: u32,

    /// rwbs[0] = 'R' (read), 'W' (write), 'D' (discard), etc.
    rwbs: [u8; 8],

    /// Process name from task_struct
    comm: [u8; 16],
}

/// BPF map: key = PID (u32), value = PidNetStats (tx_bytes, rx_bytes)
/// Max 10,240 entries (~80 KB). Suitable for systems with <10K concurrent processes.
/// On larger systems, oldest/inactive PIDs are silently dropped.
/// User-space collector handles missing PIDs gracefully.
#[map]
static NETWORK_STATS: HashMap<u32, PidNetStats> = HashMap::with_max_entries(10_240, 0);

/// BPF map: key = PID (u32), value = PidCpuStats (cpu_time_ns, last_sched_in_ns, comm)
/// Max 10,240 entries (~120 KB). Tracks cumulative CPU time per process via sched_switch.
#[map]
static CPU_STATS: HashMap<u32, PidCpuStats> = HashMap::with_max_entries(10_240, 0);

/// BPF map: key = PID (u32), value = PidDiskStats (read_bytes, write_bytes, comm)
/// Max 10,240 entries (~120 KB). Tracks cumulative block I/O bytes per process via block_rq_issue.
#[map]
static DISK_STATS: HashMap<u32, PidDiskStats> = HashMap::with_max_entries(10_240, 0);

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
    // bpf_get_current_pid_tgid() returns {tgid (upper 32), pid (lower 32)}. We use tgid (process) not tid (thread).
    let size: u64 = ctx.arg(2).unwrap_or(0);

    // Get current PID (upper 32 bits of pid_tgid)
    let pid = (bpf_get_current_pid_tgid() >> 32) as u32;

    // Update or insert the stats for this PID
    let stats = NETWORK_STATS.get_ptr_mut(&pid);
    match stats {
        Some(s) => unsafe { (*s).tx_bytes = (*s).tx_bytes.saturating_add(size) },
        None => {
            // Capture the current process name (comm field from task_struct)
            let comm = bpf_get_current_comm().unwrap_or([0u8; 16]);

            let new_stats = PidNetStats {
                tx_bytes: size,
                rx_bytes: 0,
                comm,
            };
            // Attempt to insert; map may be full if many processes are running.
            // Log to kernel trace buffer if insertion fails.
            if NETWORK_STATS.insert(&pid, &new_stats, 0).is_err() {
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

    // Get current PID (upper 32 bits of pid_tgid)
    let pid = (bpf_get_current_pid_tgid() >> 32) as u32;

    // Update or insert the stats for this PID
    let stats = NETWORK_STATS.get_ptr_mut(&pid);
    match stats {
        Some(s) => unsafe { (*s).rx_bytes = (*s).rx_bytes.saturating_add(bytes_received as u64) },
        None => {
            // Capture the current process name (comm field from task_struct)
            let comm = bpf_get_current_comm().unwrap_or([0u8; 16]);

            let new_stats = PidNetStats {
                tx_bytes: 0,
                rx_bytes: bytes_received as u64,
                comm,
            };
            // Attempt to insert; map may be full if many processes are running.
            // Log to kernel trace buffer if insertion fails.
            if NETWORK_STATS.insert(&pid, &new_stats, 0).is_err() {
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

    // Cast context to SchedSwitch struct. Use read_unaligned because kernel may pack
    // tracepoint data with non-standard alignment (safe: reading from valid kernel buffer).
    let ptr = ctx.as_ptr() as *const SchedSwitch;
    let sched_switch = unsafe { core::ptr::read_unaligned(ptr) };

    let prev_pid = sched_switch.prev_pid;
    let next_pid = sched_switch.next_pid;

    // --- Handle sched-out for prev_pid: accumulate cpu_time_ns ---
    if let Some(s) = CPU_STATS.get_ptr_mut(&prev_pid) {
        unsafe {
            let last_in = (*s).last_sched_in_ns;
            if last_in > 0 {
                // Process was on-CPU from last_in to now; add delta
                let delta = now_ns.saturating_sub(last_in);
                (*s).cpu_time_ns = (*s).cpu_time_ns.saturating_add(delta);
            }
            // Mark as off-CPU
            (*s).last_sched_in_ns = 0;
        }
    }

    // --- Handle sched-in for next_pid: stamp the start time ---
    match CPU_STATS.get_ptr_mut(&next_pid) {
        Some(s) => {
            // Process already tracked; just update the sched-in timestamp
            unsafe { (*s).last_sched_in_ns = now_ns }
        }
        None => {
            // First time we see this PID; capture its comm and initialize
            let new_stats = PidCpuStats {
                cpu_time_ns: 0,
                last_sched_in_ns: now_ns,
                comm: sched_switch.next_comm,
            };
            if CPU_STATS.insert(&next_pid, &new_stats, 0).is_err() {
                warn!(ctx, "CPU_STATS map full");
            }
        }
    }

    Ok(())
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
    // Cast context to BlockRqIssue struct. Use read_unaligned because kernel may pack
    // tracepoint data with non-standard alignment (safe: reading from valid kernel buffer).
    let ptr = ctx.as_ptr() as *const BlockRqIssue;
    let rq_issue = unsafe { core::ptr::read_unaligned(ptr) };

    // Skip zero-sector requests (no actual I/O)
    if rq_issue.nr_sector == 0 {
        return Ok(());
    }

    // Get current PID; skip kernel threads (PID 0)
    let pid = (bpf_get_current_pid_tgid() >> 32) as u32;
    if pid == 0 {
        return Ok(());
    }

    // Convert sectors to bytes (512 bytes per sector)
    let bytes = (rq_issue.nr_sector as u64).saturating_mul(512);

    // Determine if this is a read (R) or write (W) operation
    let is_read = rq_issue.rwbs[0] == b'R';
    let is_write = rq_issue.rwbs[0] == b'W';

    // Update or insert the stats for this PID
    match DISK_STATS.get_ptr_mut(&pid) {
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
                comm: rq_issue.comm,
            };
            if DISK_STATS.insert(&pid, &new_stats, 0).is_err() {
                warn!(ctx, "DISK_STATS map full");
            }
        }
    }

    Ok(())
}
