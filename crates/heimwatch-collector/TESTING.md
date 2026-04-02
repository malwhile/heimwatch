# Testing Guide: NetworkCollector eBPF Integration

This document explains the integration tests for eBPF-based network monitoring and how to run them.

## Test Overview

The test suite (`tests/network_integration.rs`) verifies critical eBPF correctness properties:

### 1. **Multi-threaded App Aggregation** ✅ (Runnable now)
- **File**: `tests/network_integration.rs:test_multithreaded_app_aggregation`
- **Status**: `#[ignore]` (requires CAP_BPF + network access)
- **Scenario**: A single application with multiple threads makes network requests.
- **Expected**: All threads' traffic aggregates under a single PID entry in the BPF map, producing one `MetricRecord` per app, not per thread.
- **Why it matters**: BPF tracks by PID, not thread ID. Verifies the kernel-side behavior is thread-agnostic.

### 2. **Large Transfer Saturation** ✅ (Runnable now)
- **File**: `tests/network_integration.rs:test_large_transfer_saturation`
- **Status**: Runnable without CAP_BPF
- **Scenario**: Simulate a process transferring >4GB of data.
- **Expected**: Byte counters saturate at `u64::MAX` without overflow or wrapping.
- **Why it matters**: Large transfers (e.g., 100GB files) must not corrupt state via overflow. `saturating_add` prevents this.

### 3. **Map Saturation & Graceful Degradation** ✅ (Runnable now)
- **File**: `tests/network_integration.rs:test_map_saturation_graceful_degradation`
- **Status**: `#[ignore]` (requires spawning 10,240+ processes)
- **Scenario**: Spawn more processes than the NETWORK_STATS map can hold (max 10,240 PIDs).
- **Expected**: BPF logs a warning to the kernel trace buffer when map is full; continues tracking other PIDs; does not panic.
- **Why it matters**: Container deployments may exceed 10K PIDs. Graceful degradation is safer than crashing.

### 4. **PID Reuse & Delta Calculation** ✅ (Runnable now)
- **File**: `tests/network_integration.rs:test_pid_reuse_saturation_delta`
- **Status**: Runnable without CAP_BPF
- **Scenario**: A process (PID 1234) transfers 1000 bytes, then exits. A new process reuses PID 1234, transfers only 100 bytes.
- **Expected**: `NetworkCollector::collect()` detects `current (100) < prev (1000)` and calculates delta as 0 (saturating_sub).
- **Why it matters**: PID reuse is common in long-running systems. Without wraparound detection, old traffic could be double-counted.

### 5. **Normal Delta Calculation** ✅ (Runnable now)
- **File**: `tests/network_integration.rs:test_normal_delta_calculation`
- **Status**: Runnable without CAP_BPF
- **Scenario**: Same app makes requests across two polls. First poll: 1000 TX. Second poll: 1500 TX.
- **Expected**: Delta = 1500 - 1000 = 500. One `MetricRecord` emitted with delta.
- **Why it matters**: Happy path. Ensures normal traffic is correctly accumulated.

### 6. **Multi-PID Aggregation by App Name** ✅ (Runnable now)
- **File**: `tests/network_integration.rs:test_multi_pid_aggregation_by_app_name`
- **Status**: Runnable without CAP_BPF
- **Scenario**: One app (e.g., Chrome) spawns 3 child processes (PIDs 1001, 1002, 1003). Each makes network requests.
- **Expected**: All 3 PIDs aggregate into a single "chrome" app entry: TX = 100+150+50 = 300.
- **Why it matters**: Apps like Chrome, Node.js, or Go servers fork child processes. User-space must aggregate them.

### 7. **Missing PID Handling** ✅ (Runnable now)
- **File**: `tests/network_integration.rs:test_missing_pid_handling`
- **Status**: Runnable without CAP_BPF
- **Scenario**: BPF map contains an entry for PID 9999, but the process has exited (no `/proc/9999` entry).
- **Expected**: User-space skips the PID silently (error in `get_process_name()`), doesn't crash or lose other data.
- **Why it matters**: Exited processes are normal. Graceful skipping prevents data loss from other PIDs.

## Running the Tests

### Quick Test (No Privileges Required)
Run the 5 fast tests that don't require CAP_BPF:

```bash
cargo test -p heimwatch-collector --test network_integration
```

**Expected output:**
```
test result: ok. 5 passed; 0 failed; 2 ignored; 0 measured
```

### Run All Tests Including Ignored Ones
To run tests marked `#[ignore]`, include the `--ignored` flag:

```bash
cargo test -p heimwatch-collector --test network_integration -- --ignored
```

**Note**: The following tests require CAP_BPF/root:
- `test_multithreaded_app_aggregation` — needs actual BPF program load + network activity
- `test_map_saturation_graceful_degradation` — needs to spawn 10,240+ processes

### Run with Full Output
For debugging, use `--nocapture`:

```bash
cargo test -p heimwatch-collector --test network_integration -- --nocapture
```

## Test Architecture

Each test follows a pattern:

1. **Setup**: Create BPF map state or network conditions
2. **Simulate**: Call the logic being tested (delta calc, aggregation, etc.)
3. **Verify**: Assert expected behavior

Tests avoid:
- Spawning real processes (too slow, environment-dependent)
- Actual network calls (CI might block network, or we'd need test servers)
- Root/CAP_BPF dependencies (except for `#[ignore]` tests)

Instead, they simulate BPF map state directly and verify the user-space collection logic.

## Adding New Tests

To add a test for a new scenario:

```rust
#[test]
fn test_my_scenario() {
    // Setup: create initial state
    let prev = ...;
    let current = ...;
    
    // Simulate: apply the logic
    let delta = current.saturating_sub(prev);
    
    // Verify: assert expected behavior
    assert_eq!(delta, expected);
    println!("✓ Description of what passed");
}
```

For tests requiring CAP_BPF or 10K processes, use:

```rust
#[test]
#[ignore] // Requires CAP_BPF or special setup
fn test_my_special_case() {
    // ...
}
```

## CI Integration

These tests run in CI via:

```bash
make test
```

Only the 5 non-ignored tests run by default (no CAP_BPF in CI).

The `#[ignore]` tests are documented here for local development and can be run manually on a developer machine with capabilities:

```bash
# Grant capabilities to the target binary
sudo setcap cap_bpf,cap_perfmon+ep ./target/debug/heimwatch

# Then run ignored tests
cargo test -p heimwatch-collector --test network_integration -- --ignored --nocapture
```

## Real-World Integration Testing

For end-to-end testing (beyond these unit-like tests):

1. **Start the daemon** (requires CAP_BPF or root):
   ```bash
   sudo setcap cap_bpf,cap_perfmon+ep ./target/debug/heimwatch
   ./target/debug/heimwatch --db /tmp/test.db
   ```

2. **Generate traffic** from another terminal:
   ```bash
   curl https://example.com
   ping 8.8.8.8
   ```

3. **Verify storage** after 5-10 seconds:
   ```bash
   # Check stored metrics (depends on final storage API)
   ```

## Common Failure Modes

### Test Failure: PID Reuse Detection
If `test_pid_reuse_saturation_delta` fails with assertion:
```
assertion `left == right` failed: left = 100, right = 0
```
This means the delta is not 0 when it should be. Check:
- Verify `saturating_sub(current, prev)` returns 0 when `current < prev`
- Ensure app names match between prev/current (aggregation key)

### Test Failure: Aggregation
If `test_multi_pid_aggregation_by_app_name` fails:
- Verify PIDs are being summed correctly with `saturating_add`
- Ensure the app_name resolution logic matches all 3 PIDs to "chrome"

### Missing Tests
The two `#[ignore]` tests require:
- `test_multithreaded_app_aggregation`: Real multi-threaded process + BPF load
- `test_map_saturation_graceful_degradation`: 10K+ process spawning

These are documented but not part of regular CI. They should be run:
- Before merging changes to delta calculation logic
- When modifying map size limits
- When stress-testing the system

## Related Issues

- **Issue #139**: eBPF kernel-space implementation (merged)
- **Issue #140**: NetworkCollector user-space implementation (in progress)
- **Issue #141**: Daemon integration (in progress)

These tests verify correctness of Issue #139 and ensure Issue #140/141 implementations work correctly.
