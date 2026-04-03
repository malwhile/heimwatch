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

use clap::Parser;
use heimwatch_daemon::logging::{LogConfig, init_logging, parse_level};
use heimwatch_daemon::run;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(name = "heimwatch-daemon")]
#[command(about = "Heimwatch system monitoring daemon", long_about = None)]
struct Args {
    /// Poll interval in seconds
    #[arg(short, long, default_value = "5")]
    interval: u64,

    /// Database path (sled)
    #[arg(short, long, default_value = "./heimwatch.db")]
    db: String,

    /// Log level (off, error, warn, info, debug, trace)
    #[arg(long, default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Initialize centralized logging before running any crates
    let log_level = parse_level(&args.log_level)?;
    let log_config = LogConfig::new(log_level);
    init_logging(log_config)?;

    log::info!(
        "Heimwatch daemon starting with poll_interval={}s, db={}",
        args.interval,
        args.db
    );

    let poll_interval = Duration::from_secs(args.interval);
    run(poll_interval, &args.db).await?;

    Ok(())
}
