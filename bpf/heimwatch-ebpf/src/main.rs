#![no_std]
#![no_main]

use aya_ebpf::{
    helpers::{bpf_get_current_comm, bpf_get_current_pid_tgid, bpf_ktime_get_ns},
    macros::{kprobe, kretprobe, map, tracepoint},
    maps::HashMap,
    programs::{ProbeContext, RetProbeContext, TracePointContext},
};
use aya_log_ebpf::warn;
use heimwatch_ebpf_common::{PidNetStats, PidCpuStats};

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe { core::hint::unreachable_unchecked() }
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

/// Attached to tcp_sendmsg. Size is passed directly as arg 2.
///
/// // arg(2) contains the send size; args 0-1 are kernel struct pointers (inaccessible from BPF)
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

/// Attached to tcp_recvmsg (kretprobe). Reads the return value (bytes received)
/// and accumulates rx_bytes for the current PID.
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
/// the process being switched off (prev). We read next_pid from the tracepoint context.
///
/// sched_switch tracepoint memory layout (stable since kernel 4.4):
///   offset  0: u64       common header
///   offset  8: [u8; 16]  prev_comm
///   offset 24: u32       prev_pid
///   offset 28: i32       prev_prio
///   offset 32: i64       prev_state
///   offset 40: [u8; 16]  next_comm
///   offset 56: u32       next_pid
///
/// Before production deployment, verify these offsets on the target system:
///   cat /sys/kernel/debug/tracing/events/sched/sched_switch/format
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

    // Read prev_pid and next_pid from tracepoint context
    let prev_pid: u32 = unsafe { ctx.read_at(24)? };
    let next_pid: u32 = unsafe { ctx.read_at(56)? };

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
            let comm: [u8; 16] = unsafe { ctx.read_at(40).unwrap_or([0u8; 16]) };
            let new_stats = PidCpuStats {
                cpu_time_ns: 0,
                last_sched_in_ns: now_ns,
                comm,
            };
            if CPU_STATS.insert(&next_pid, &new_stats, 0).is_err() {
                warn!(ctx, "CPU_STATS map full");
            }
        }
    }

    Ok(())
}
