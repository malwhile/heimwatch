//! Heimwatch daemon entry point.
//!
//! # Capabilities Required
//!
//! For network monitoring on Linux (eBPF), the daemon needs:
//!
//! ```bash
//! sudo setcap cap_bpf,cap_perfmon+ep ./target/release/heimwatch-daemon
//! ```
//!
//! Or run with `sudo`.

use clap::{Parser, Subcommand};
use heimwatch_daemon::logging::{LogConfig, init_logging, parse_level};
use heimwatch_daemon::{run, snapshot};
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(name = "heimwatch-daemon")]
#[command(about = "Heimwatch system monitoring daemon")]
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
        /// Poll interval in seconds
        #[arg(short, long, default_value = "5")]
        interval: u64,

        /// Database path (sled)
        #[arg(short, long, default_value = "./heimwatch.db")]
        db: String,
    },
    /// Capture a one-shot traffic snapshot and print to stdout
    Snapshot {
        /// Observation window in seconds (probes attach, traffic accumulates, then results print)
        #[arg(short, long, default_value = "3")]
        window: u64,

        /// Output format: text (human-readable) or json
        #[arg(short, long, default_value = "text")]
        format: String,
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
        Command::Daemon { interval, db } => {
            log::info!(
                "Heimwatch daemon starting with poll_interval={}s, db={}",
                interval,
                db
            );
            let poll_interval = Duration::from_secs(interval);
            run(poll_interval, &db).await?;
        }
        Command::Snapshot { window, format } => {
            snapshot::run_snapshot(window, &format).await?;
        }
    }

    Ok(())
}
