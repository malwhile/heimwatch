#![no_std]
#![no_main]

use aya_ebpf::{
    helpers::bpf_get_current_pid_tgid,
    macros::{kprobe, kretprobe, map},
    maps::HashMap,
    programs::{ProbeContext, RetProbeContext},
};
use aya_log_ebpf::warn;
use heimwatch_ebpf_common::PidNetStats;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe { core::hint::unreachable_unchecked() }
}

/// BPF map: key = PID (u32), value = PidNetStats (tx_bytes, rx_bytes)
/// Max 10,240 entries (~80 KB). Suitable for systems with <10K concurrent processes.
/// On larger systems, oldest/inactive PIDs are silently dropped.
/// User-space collector handles missing PIDs gracefully.
#[map]
static NETWORK_STATS: HashMap<u32, PidNetStats> =
    HashMap::with_max_entries(10_240, 0);

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
            let new_stats = PidNetStats {
                tx_bytes: size,
                rx_bytes: 0,
            };
            // Attempt to insert; map may be full if many processes are running.
            // Log to kernel trace buffer if insertion fails.
            if NETWORK_STATS.insert(&pid, &new_stats, 0).is_err() {
                warn!(ctx, "NETWORK_STATS map full; cannot track PID {}", pid);
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
        Some(s) => {
            unsafe { (*s).rx_bytes = (*s).rx_bytes.saturating_add(bytes_received as u64) }
        }
        None => {
            let new_stats = PidNetStats {
                tx_bytes: 0,
                rx_bytes: bytes_received as u64,
            };
            // Attempt to insert; map may be full if many processes are running.
            // Log to kernel trace buffer if insertion fails.
            if NETWORK_STATS.insert(&pid, &new_stats, 0).is_err() {
                warn!(ctx, "NETWORK_STATS map full; cannot track PID {}", pid);
            }
        }
    }

    Ok(())
}
