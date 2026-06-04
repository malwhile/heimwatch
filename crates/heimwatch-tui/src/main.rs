use clap::Parser;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(name = "heimwatch-tui")]
#[command(about = "Heimwatch terminal user interface")]
struct Args {
    #[arg(short, long, default_value = "./heimwatch.db")]
    db: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let args = Args::parse();

    let storage = Arc::new(heimwatch_storage::StorageLayer::open(&args.db)?);

    heimwatch_tui::run(storage, args.db).await
}
