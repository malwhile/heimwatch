# Research: Network Usage Monitoring on Linux

## Goal
Track network connections and data volume (bytes sent/received) per application with low overhead.

## Options Investigated

### 1. Polling `/proc/[pid]/net/tcp` and `/proc/net/dev`
- **Mechanism:** Read socket files in `/proc/[pid]/fd/` to find inodes, then match inodes in `/proc/net/tcp`.
- **Pros:** No root required. Very fast.
- **Cons:** **Does not provide byte counters.** Only shows connection state (ESTABLISHED, TIME_WAIT). Cannot calculate volume without eBPF or libpcap.
- **Verdict:** Good for "Active Connections" count, useless for "Data Volume".

### 2. Libpcap (Packet Capture)
- **Mechanism:** Capture every packet and sum bytes by PID (using `SO_PEERCRED` or socket options).
- **Pros:** Accurate volume.
- **Cons:** **High overhead.** Requires root (`CAP_NET_RAW`). Heavy CPU usage if parsing every packet.
- **Verdict:** Too heavy for a background daemon.

### 3. eBPF (Extended Berkeley Packet Filter)
- **Mechanism:** Attach a kernel probe (kprobe/uprobe) to `sock_sendmsg` and `sock_recvmsg`. Count bytes per PID in the kernel.
- **Pros:** **Near-zero overhead.** Accurate per-PID volume. No need to parse packets in user space.
- **Cons:** Requires kernel support (4.x+). Requires `CAP_BPF` or `CAP_PERFMON` (often requires root or `setcap` on the binary).
- **Verdict:** **The Best Choice.** Modern, efficient, and accurate.

## Recommended Approach: eBPF with `aya`

We will use the `aya` crate to attach a BPF program that counts bytes sent/received per PID.

### Architecture
1.  **User Space (Rust):**
    - Loads the BPF bytecode.
    - Reads the map of `{ PID: { tx_bytes, rx_bytes } }`.
    - Calculates deltas between polls.
2.  **Kernel Space (BPF):**
    - Hooks `sock_sendmsg` / `sock_recvmsg`.
    - Increments counters in a BPF map.

### Dependencies
- `aya` (for BPF loading/management)
- `libc` (for syscall helpers)
- `procfs` (to map PIDs to process names)

### Privileges
- Requires `CAP_BPF` or `CAP_PERFMON`.
- **Solution:** Ship the binary with `sudo setcap cap_bpf,cap_perfmon+ep ./heimwatch` or require `sudo` for the first run.

## Historical Data & Deltas

### How to Calculate Volume
Since BPF gives us the **current total** bytes for a PID (since boot or since the map was cleared), we must calculate the **delta**.

1.  **Poll Interval:** Every `N` seconds (e.g., 5s).
2.  **Read Map:** Get `tx_bytes` and `rx_bytes` for all PIDs.
3.  **Store Previous State:** Keep a `HashMap<Pid, Bytes>` in memory.
4.  **Calculate Delta:** `Current - Previous = Bytes in this interval`.
5.  **Store:** Write the delta to `sled` with a timestamp.
6.  **Update Previous:** `Previous = Current`.

### Handling Process Restart
If a PID dies and a new process gets the same PID:
- The BPF map will show the new process's bytes (which might be 0 or inherited if the kernel reuses the slot, but usually BPF maps are keyed by PID, so it resets or overwrites).
- **Logic:** If `Current < Previous`, assume the process restarted. Reset `Previous` to `Current` (or 0 if the map is cleared).

## Initial implemntation plan
1.  Create a minimal BPF program using `aya` to count bytes.
2.  Loop through all running PIDs
3.  Get process/app name from PID
    -  Use process/app name for storing the deltas
    -  This should eliminate the possibility of overlapping PIDs since if the PID is different, the process/app name should be the same
4.  Using PIDs query the BPF program for bytes
5.  Calculate the deltas

## Conclusion
**eBPF is the only viable path** for accurate, low-overhead, per-application network volume monitoring on Linux. Polling `/proc` is insufficient for volume.
