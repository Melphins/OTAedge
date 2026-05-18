use agent::EdgeAgent;
use anyhow::Result;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    device_id: String,

    #[arg(short, long)]
    server_url: String,

    #[arg(short, long)]
    token: String,

    #[arg(short, long, default_value = "./models")]
    model_dir: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    let config = agent::Config {
        token: args.token,
        heartbeat_interval: 30,
        model_cache_dir: args.model_dir,
    };

    let mut agent = EdgeAgent::new(args.device_id, args.server_url, config);
    agent.run().await?;

    Ok(())
}
