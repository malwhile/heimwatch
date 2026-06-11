//! Heimwatch daemon entry point.
//!
//! # Capabilities Required
//!
//! For network monitoring and CPU tracking on Linux (eBPF), the daemon needs:
//!
//! ```bash
//! sudo setcap cap_bpf,cap_perfmon+ep ./target/release/heimwatch-daemon
//! ```
//!
//! Or run with `sudo`.

use std::fs;

use clap::{Parser, Subcommand};
use heimwatch_daemon::logging::{LogConfig, init_logging, parse_level};
use heimwatch_daemon::{run, snapshot, test_data};

#[derive(Parser, Debug)]
#[command(name = "heimwatch")]
#[command(about = "Heimwatch system monitoring daemon and terminal interface")]
struct Args {
    /// Log level (off, error, warn, info, debug, trace)
    #[arg(long, global = true, default_value = "info")]
    log_level: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run as a continuous background daemon
    Daemon {
        /// Database path (sled)
        #[arg(short, long, default_value = "./heimwatch.db")]
        db: String,

        /// Configuration file path (TOML)
        #[arg(short, long, default_value = "./heimwatch.toml")]
        config: String,
    },
    /// View metrics in the terminal interface
    Tui {
        /// Database path (sled)
        #[arg(short, long, default_value = "./heimwatch.db")]
        db: String,
    },
    /// Generate test data for manual TUI testing
    TestData {
        /// Database path (sled)
        #[arg(short, long, default_value = "./heimwatch.db")]
        db: String,

        /// Hours of historical data to generate
        #[arg(long, default_value = "24")]
        hours: u32,

        /// Records per app to generate (distributed across the hours)
        #[arg(long, default_value = "20")]
        records: u32,
    },
    /// Capture a snapshot and print to stdout
    #[command(subcommand)]
    Snapshot(SnapshotCommand),
}

#[derive(Subcommand, Debug)]
enum SnapshotCommand {
    /// One-shot CPU usage snapshot (attaches eBPF sched_switch probe)
    Cpu {
        /// Observation window in seconds (probe attaches, CPU time accumulates, then collect)
        #[arg(short, long, default_value = "5")]
        window: u64,

        /// Output format: text (human-readable) or json
        #[arg(short, long, default_value = "text")]
        format: String,
    },
    /// One-shot network traffic snapshot (attaches eBPF probes)
    Network {
        /// Observation window in seconds (probes attach, traffic accumulates, then collect)
        #[arg(short, long, default_value = "3")]
        window: u64,

        /// Output format: text (human-readable) or json
        #[arg(short, long, default_value = "text")]
        format: String,
    },
    /// One-shot disk I/O snapshot (attaches eBPF block_rq_issue probe)
    Disk {
        /// Observation window in seconds (probe attaches, disk bytes accumulate, then collect)
        #[arg(short, long, default_value = "5")]
        window: u64,

        /// Output format: text (human-readable) or json
        #[arg(short, long, default_value = "text")]
        format: String,
    },
    /// One-shot memory usage snapshot (polls /proc/[pid]/status)
    Memory {
        /// Observation window in seconds (polling accumulates, then collect)
        #[arg(short, long, default_value = "5")]
        window: u64,

        /// Output format: text (human-readable) or json
        #[arg(short, long, default_value = "text")]
        format: String,
    },
    /// One-shot GPU metrics snapshot (polls sysfs and hwmon)
    Gpu {
        /// Observation window in seconds (polling collects metrics)
        #[arg(short, long, default_value = "5")]
        window: u64,

        /// Output format: text (human-readable) or json
        #[arg(short, long, default_value = "text")]
        format: String,
    },
    /// Focus time snapshot (queries database for focus events)
    Focus {
        /// Query window in seconds (e.g., last 30 seconds of focus data)
        #[arg(short, long, default_value = "30")]
        window: u64,

        /// Output format: text (human-readable) or json
        #[arg(short, long, default_value = "text")]
        format: String,

        /// Database path (required for focus snapshot)
        #[arg(short, long)]
        db: String,
    },
    /// Power usage snapshot (queries database for per-app power attribution)
    Power {
        /// Query window in seconds (default: last hour)
        #[arg(short, long, default_value = "3600")]
        window: u64,

        /// Output format: text (human-readable) or json
        #[arg(short, long, default_value = "text")]
        format: String,

        /// Database path (required for power snapshot)
        #[arg(short, long)]
        db: String,

        /// Maximum number of apps to show per section
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Initialize centralized logging before running any crates
    let log_level = parse_level(&args.log_level)?;
    let log_config = LogConfig::new(log_level);
    init_logging(log_config)?;

    match args.command {
        Command::Daemon { db, config } => {
            log::info!(
                "Heimwatch daemon starting with db={}, config={}, config status={}",
                db,
                config,
                if fs::exists(&config).is_ok() {
                    "loaded from file"
                } else {
                    "file not found, loading defaults"
                }
            );
            run(&db, Some(&config)).await?;
        }
        Command::Tui { db } => {
            let storage = std::sync::Arc::new(
                heimwatch_storage::StorageLayer::open(&db)
                    .map_err(|e| anyhow::anyhow!("Failed to open database: {}", e))?,
            );
            heimwatch_tui::run(storage, db).await?;
        }
        Command::TestData { db, hours, records } => {
            log::info!(
                "Generating {} records per app over {} hours in {}",
                records,
                hours,
                db
            );
            test_data::generate_test_data(&db, hours, records)?;
            println!("Test database created at: {}", db);
            println!(
                "Run `cargo run -p heimwatch-daemon -- tui --db {}` to view it",
                db
            );
        }
        Command::Snapshot(snapshot_cmd) => match snapshot_cmd {
            SnapshotCommand::Cpu { window, format } => {
                snapshot::run_snapshot(window, &format, "cpu", None).await?;
            }
            SnapshotCommand::Network { window, format } => {
                snapshot::run_snapshot(window, &format, "network", None).await?;
            }
            SnapshotCommand::Disk { window, format } => {
                snapshot::run_snapshot(window, &format, "disk", None).await?;
            }
            SnapshotCommand::Memory { window, format } => {
                snapshot::run_snapshot(window, &format, "memory", None).await?;
            }
            SnapshotCommand::Gpu { window, format } => {
                snapshot::run_snapshot(window, &format, "gpu", None).await?;
            }
            SnapshotCommand::Focus { window, format, db } => {
                snapshot::run_snapshot(window, &format, "focus", Some(&db)).await?;
            }
            SnapshotCommand::Power {
                window,
                format,
                db,
                limit,
            } => {
                snapshot::run_power_snapshot(window, &format, &db, limit).await?;
            }
        },
    }

    Ok(())
}
