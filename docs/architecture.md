# Architectural Decisions Record (ADR) - Heimwatch

> **Status:** Draft (v0.1)  
> **Last Updated:** 2026-03-31  
> **Author:** Heimwatch Team

## 1. Executive Summary

Heimwatch is a privacy-first, cross-platform system monitoring daemon designed to provide "Android Screen Time"-style analytics for desktop operating systems. The architecture prioritizes **modularity**, **low overhead**, and **data sovereignty**.

The system is built as a **Rust Workspace** to enforce strict module boundaries, allowing independent development of OS-specific collectors while sharing a common core.

---

## 2. Core Design Principles

1.  **Privacy by Default**: All data is stored locally. No telemetry is sent to external servers.
2.  **Zero-Trust Architecture**: The daemon runs with minimal privileges; sensitive data (like window titles) is processed in memory and only persisted if explicitly configured.
3.  **Modular Extensibility**: New OS support (macOS, Windows, BSD) requires implementing a trait, not rewriting the core logic.
4.  **Low Overhead**: The daemon targets <1% CPU usage and <50MB RAM footprint during idle monitoring.

---

## 3. High-Level Architecture

The system follows a **Producer-Consumer** pattern with a **Plugin-like** OS abstraction layer.

```mermaid
graph TD
    subgraph "OS Abstraction Layer (Traits)"
        A[Linux Collector] -->|Implements| C(Collector Trait)
        B[macOS Collector] -->|Implements| C
        D[Windows Collector] -->|Implements| C
        E[BSD Collector] -->|Implements| C
    end

    subgraph "Core Logic"
        C --> F[Data Aggregator]
        F --> G[Alert Engine]
        G --> H[Storage Layer]
    end

    subgraph "Interfaces"
        H --> I[Web Dashboard (Axum)]
        H --> J[TUI (Ratatui)]
    end

    K[User Config] --> G
    K --> H
```

## 4. Module Breakdown

The project is structured as a Cargo Workspace with the following crates:

| Crate | Responsibility | Key Dependencies |
| ----- | -------------- | ---------------- |
| heimwatch-core | Shared types, traits, and OS abstraction interfaces. Defines the contract for collectors. | serde, chrono |
| heimwatch-collector | OS-specific implementations (Linux/Wayland currently). Handles raw data retrieval (network, power, focus). | procfs, wayland-client, sysinfo |
| heimwatch-storage | Time-series data persistence using an embedded KV store. Handles retention policies. | sled, serde\_json |
| heimwatch-web | Lightweight HTTP server serving the dashboard. Exposes REST/SSE endpoints. | axum, tower-http |
| heimwatch-tui | Terminal-based interface for SSH/low-resource environments. | ratatui, crossterm |
| heimwatch-daemon | Service management (systemd, launchd), signal handling, and lifecycle control. | tokio, clap |

## 5. Key Technology Choices

### 5.1. Language: Rust

- Reasoning: Memory safety without garbage collection, zero-cost abstractions, and excellent cross-compilation support.
- Impact: Ensures the daemon is safe to run as a background service with minimal risk of memory leaks or crashes.

### 5.2. Database: Sled (Embedded KV Store)

- Reasoning: Chosen over SQLite for its lock-free architecture, simplicity, and lack of external dependencies.
- Schema Strategy: Keys are structured as metric\_type:timestamp:pid to enable efficient time-range scans. Values are serialized JSON blobs.
- Trade-off: Less SQL-like querying power than SQLite, but faster for append-heavy time-series workloads.

### 5.3. Web Interface: Axum + HTMX + Chart.js

- Reasoning:
    - Axum: Ergonomic, high-performance Rust web framework.
    - HTMX: Allows dynamic UI updates without heavy JavaScript frameworks (React/Vue), keeping the frontend lightweight.
    - Chart.js: Standard library for rendering time-series graphs.
- Deployment: Static assets are embedded directly into the binary (include_str!), resulting in a single portable executable.

### 5.4. Async Runtime: Tokio

- Reasoning: Industry standard for async Rust. Required for non-blocking I/O when monitoring network traffic and handling multiple collectors simultaneously.

### 5.5. OS Abstraction: Trait-Based

- Strategy: Define a Collector trait in heimwatch-core.

```rust
pub trait Collector {
    fn collect_network(&self) -> Result<NetworkData>;
    fn collect_power(&self) -> Result<PowerData>;
    // ...
}
```

- Implementation: Each OS (Linux, macOS, etc.) provides its own implementation of this trait. The main binary selects the correct implementation via feature flags at compile time.

## 6. Data Flow

1. Collection: The Collector runs on a configurable interval (e.g., every 5s).
2. Aggregation: Raw data is normalized into a standard Metric struct.
3. Storage: Data is batch-written to sled to minimize disk I/O.
4. Retrieval: The Web/TUI interfaces request time-range data via the Storage layer.
5. Alerting: The Alert Engine continuously evaluates new metrics against user-defined rules.

## 7. Future Considerations & Risks

- Wayland Compatibility: Wayland security model restricts window focus tracking. We rely on Desktop Portals or specific compositor APIs (Sway/KDE/GNOME). Risk: Some compositors may not support focus tracking.
- Power Estimation: Accurate per-app power usage is hardware-dependent (RAPL). Mitigation: Graceful degradation to system-wide estimates if per-app data is unavailable.
- Cross-Platform Parity: Windows/macOS APIs differ significantly from Linux. Mitigation: Strict adherence to the Collector trait to isolate complexity.

## 8. Revision History

| Version | Date | Author | Changes |
| 0.1 | 2026-03-31 | malwhile | Initial architectural draft. |
