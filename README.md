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

## 🏗️ Architecture

Heimwatch is built as a modular Rust workspace:

- **`heimwatch-core`**: Shared types, traits, and OS abstraction layers.
- **`heimwatch-collector`**: OS-specific data collection (Network, Power, Focus, etc.).
- **`heimwatch-storage`**: Time-series data persistence using `sled`.
- **`heimwatch-web`**: Lightweight Axum server serving the dashboard.
- **`heimwatch-tui`**: Ratatui-based terminal interface.
- **`heimwatch-daemon`**: Service management (systemd, launchd, etc.).

## 🚀 Roadmap

- [ ] **Phase 1**: Project scaffolding, CI/CD, and architecture design.
- [ ] **Phase 2**: Linux/Wayland data collection implementation.
- [ ] **Phase 3**: Sled storage layer and data retention.
- [ ] **Phase 4**: Web dashboard and real-time updates.
- [ ] **Phase 5**: TUI implementation.
- [ ] **Phase 6**: Daemon service management.
- [ ] **Phase 7**: Alerting system.
- [ ] **Phase 8**: Cross-platform expansion (macOS, Windows, BSD).
- [ ] **Phase 9**: Documentation, licensing, and v1.0 release.

## 📜 License

Licensed under the MIT License. See [LICENSE](LICENSE) for details.

## 🤝 Contributing

Contributions are welcome! Please read [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

---

*Heimwatch: Guarding your digital bridge.*
