# Heimwatch

> **The All-Seeing Eye of Your System**

Heimwatch is a lightweight, cross-platform system monitoring daemon inspired by the Norse god Heimdall. It tracks application usage, network traffic, power consumption, and system resources to provide deep insights into your digital habits and system health.

## 🎯 Vision

To provide a privacy-focused, low-overhead alternative to cloud-based screen time trackers, giving users full control over their data while offering "Android Screen Time"-style analytics for Linux, macOS, Windows, and BSD.

## ✨ Key Features

- **📊 Comprehensive Metrics**: Tracks CPU, Memory, Disk, GPU, Network, Power, and Focus Time per application.
- **🛡️ Privacy First**: All data is stored locally using an embedded database (Sled). No data leaves your machine.
- **⚡ Low Overhead**: Written in Rust with a focus on minimal CPU and memory footprint.
- **🌐 Dual Interface**:
  - **Web Dashboard**: Beautiful, real-time charts using HTMX and Chart.js.
  - **TUI**: A terminal-based interface for SSH environments and low-resource setups.
- **🔔 Smart Alerting**: Configurable alerts for usage thresholds (e.g., "Notify me if Chrome uses >50% CPU for 10 mins").
- **🌍 Cross-Platform**: Designed with a modular architecture to support Linux (Wayland), macOS, Windows, and BSD.

## 📚 Documentation

**User Documentation** is available in the [`docs/`](docs/) folder:
- [Data Retention & Cleanup](docs/RETENTION.md) — Configure automatic data retention policies, set cleanup intervals, and manage storage growth.
- Example configuration files and deployment guides (coming soon)

**Developer Documentation** is in the root:
- [CLAUDE.md](CLAUDE.md) — Architecture decisions, eBPF CO-RE implementation, and development setup for Claude Code
- This README provides a high-level overview

## 🏗️ Architecture

Heimwatch is built as a modular Rust workspace:

- **`heimwatch-core`**: Shared types, traits, and OS abstraction layers.
- **`heimwatch-collector`**: OS-specific data collection (Network, Power, Focus, etc.).
- **`heimwatch-storage`**: Time-series data persistence using `sled`, with automatic data retention and cleanup.
- **`heimwatch-web`**: Lightweight Axum server serving the dashboard.
- **`heimwatch-tui`**: Ratatui-based terminal interface.
- **`heimwatch-daemon`**: Daemon orchestration, event collection, and scheduled cleanup tasks.

## 📋 System Requirements

### Linux (Minimum)

- **Kernel Version**: Linux 4.x or later (5.4+ recommended for improved BTF/CO-RE stability across kernel versions).
- **Capabilities**: The daemon must run with either:
  - `CAP_BPF` and `CAP_PERFMON` (preferred; Linux 5.8+), OR
  - `root` privileges

  To grant capabilities to the binary without requiring root:
  ```bash
  sudo setcap cap_bpf,cap_perfmon+ep /path/to/heimwatch
  ```

- **Process Limit**: Heimwatch tracks up to 10,240 concurrent processes. Container orchestration systems (Kubernetes, Docker Swarm) that spawn large numbers of processes or containers may hit this limit. Future versions will support configurable limits; see the [roadmap](#-roadmap) for details on Phase 3+ scaling improvements.

### macOS, Windows, BSD

Not yet implemented. See [Phase 8](#-roadmap) of the roadmap.

## 👨‍💻 Developer Setup

### Building on Linux

Heimwatch uses eBPF for kernel-space network monitoring. Building requires the nightly Rust toolchain with `rust-src`.

**One-time setup:**
```bash
# Install nightly Rust toolchain with rust-src component
# Note: bpf-unknown-unknown is built from rust-src; no pre-built target exists
rustup toolchain install nightly --component rust-src

# Install bpf-linker (LLVM-based BPF linker; requires LLVM 18+)
cargo install bpf-linker
```

**Build and run:**
```bash
cargo build --release
```

The `build.rs` script in `heimwatch-collector` automatically cross-compiles the eBPF program and embeds it in the binary.

### Running the Daemon

After building, grant the necessary capabilities or run as root:

```bash
# Option 1: Grant minimal capabilities (preferred)
sudo setcap cap_bpf,cap_perfmon+ep ./target/release/heimwatch

# Then run the daemon (with optional config file)
./target/release/heimwatch daemon --db ./heimwatch.db --config ./heimwatch.toml

# Option 2: Run with sudo
sudo ./target/release/heimwatch daemon --db ./heimwatch.db
```

**Configuration**: Copy and customize the example configuration:
```bash
cp docs/heimwatch.toml.example heimwatch.toml
# Edit heimwatch.toml to adjust retention policies, cleanup intervals, etc.
```

See [Data Retention & Cleanup](docs/RETENTION.md) for configuration options.

## 🚀 Roadmap

- [x] **Phase 1**: Project scaffolding, CI/CD, and architecture design.
- [x] **Phase 2**: Linux/Wayland data collection implementation (CPU, Network, Power, Focus, Memory, Disk, GPU).
- [x] **Phase 3**: Sled storage layer, CO-RE-based eBPF probes, and automatic data retention with configurable per-metric-type cleanup policies.
- [ ] **Phase 4**: Web dashboard and real-time updates (Axum + HTMX + Chart.js).
- [ ] **Phase 5**: TUI implementation (Ratatui).
- [ ] **Phase 6**: Daemon service management (systemd, launchd).
- [ ] **Phase 7**: Alerting system (configurable usage thresholds).
- [ ] **Phase 8**: Cross-platform expansion (macOS, Windows, BSD).
- [ ] **Phase 9**: Data aggregation (hourly/daily buckets) and optimized long-term storage.
- [ ] **Phase 10**: Documentation, licensing, and v1.0 release.

## 📜 License

Licensed under the MIT License. See [LICENSE](LICENSE) for details.

## 🤝 Contributing

Contributions are welcome! Please read [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

---

*Heimwatch: Guarding your digital bridge.*
