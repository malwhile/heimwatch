# Heimwatch Documentation

This folder contains both **user documentation** and **developer design documents**.

## 📖 User Documentation

**Getting Started:**
1. See [main README](../README.md) for build and setup instructions
2. Read [Data Retention & Cleanup](RETENTION.md) to configure automatic cleanup
3. Copy and customize [heimwatch.toml.example](heimwatch.toml.example)
4. Run the daemon:
   ```bash
   ./target/release/heimwatch daemon --db ./heimwatch.db --config ./heimwatch.toml
   ```

**User Topics:**
- **[Data Retention & Cleanup](RETENTION.md)** — Configure automatic data retention policies, set cleanup intervals, manage storage growth, and export before deletion.
- **[heimwatch.toml.example](heimwatch.toml.example)** — Complete configuration file with all available options and defaults.

**Coming Soon:**
- Web dashboard configuration and usage
- TUI navigation guide
- Data export formats and analysis
- Alerting setup and configuration

## 🏗️ Developer Documentation

**Design & Architecture:**
Design documents and implementation plans are in [`design/`](design/):
- **[architecture.md](design/architecture.md)** — Overall system design and component interactions
- **[power-plan.md](design/power-plan.md)** — Power consumption measurement strategy
- **[storage-plan.md](design/storage-plan.md)** — Data persistence and aggregation design
- **[cpu-tracking-ebpf.md](design/cpu-tracking-ebpf.md)** — CPU monitoring via eBPF
- **[network-monitoring.md](design/network-monitoring.md)** — Network traffic tracking
- **[disk-tracking-ebpf.md](design/disk-tracking-ebpf.md)** — Disk I/O monitoring
- **[memory-tracking.md](design/memory-tracking.md)** — Memory usage collection
- **[gpu-metrics.md](design/gpu-metrics.md)** — GPU usage tracking
- **[app-focus-monitoring.md](design/app-focus-monitoring.md)** — Window focus detection

**For Developer Setup:**
- See [../CLAUDE.md](../CLAUDE.md) — Architecture decisions, eBPF CO-RE implementation, and development guidelines
- See [../README.md](../README.md#-developer-setup) — Build and run instructions
