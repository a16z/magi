use std::{
    str::FromStr,
    sync::mpsc::{channel, Sender},
};

use clap::Parser;
use dirs::home_dir;
use ethers::types::H256;
use eyre::Result;

use magi::{
    config::{ChainConfig, CliConfig, Config, SyncMode},
    driver::Driver,
    telemetry::{self, metrics},
};
use serde::Serialize;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let sync_mode = cli.sync_mode.clone();
    let verbose = cli.verbose;
    let logs_dir = cli.logs_dir.clone();
    let logs_rotation = cli.logs_rotation.clone();
    let checkpoint_hash = cli.checkpoint_hash.clone();
    let config = cli.to_config();

    let _guards = telemetry::init(verbose, logs_dir, logs_rotation);
    metrics::init()?;

    match sync_mode {
        SyncMode::Fast => fast_sync(config, checkpoint_hash).await?,
        SyncMode::Full => full_sync(config).await?,
        SyncMode::Challenge => panic!("challenge sync not implemented"),
    };

    Ok(())
}

pub async fn full_sync(config: Config) -> Result<()> {
    tracing::info!(target: "magi", "starting full sync");
    let (shutdown_sender, shutdown_recv) = channel();
    shutdown_on_ctrlc(shutdown_sender);

    let mut driver = Driver::from_config(config, shutdown_recv, None).await?;

    if let Err(err) = driver.start().await {
        tracing::error!(target: "magi", "{}", err);
        std::process::exit(1);
    }

    Ok(())
}

pub async fn fast_sync(config: Config, checkpoint_hash: Option<String>) -> Result<()> {
    tracing::info!(target: "magi", "starting fast sync");
    let (shutdown_sender, shutdown_recv) = channel();
    shutdown_on_ctrlc(shutdown_sender);

    let checkpoint_hash =
        H256::from_str(&checkpoint_hash.expect(
            "fast sync requires an L2 block hash to be provided as checkpoint_hash for now",
        ))
        .expect("invalid checkpoint hash provided");

    let mut driver = Driver::from_config(config, shutdown_recv, Some(checkpoint_hash)).await?;

    if let Err(err) = driver.start_fast().await {
        tracing::error!(target: "magi", "{}", err);
        std::process::exit(1);
    }

    Ok(())
}

#[derive(Parser, Serialize)]
pub struct Cli {
    #[clap(short, long, default_value = "optimism-goerli")]
    network: String,
    #[clap(long)]
    l1_rpc_url: Option<String>,
    #[clap(long)]
    l2_rpc_url: Option<String>,
    #[clap(short = 'm', long, default_value = "full")]
    sync_mode: SyncMode,
    #[clap(long)]
    l2_engine_url: Option<String>,
    #[clap(long)]
    jwt_secret: Option<String>,
    #[clap(short = 'v', long)]
    verbose: bool,
    #[clap(long)]
    logs_dir: Option<String>,
    #[clap(long)]
    logs_rotation: Option<String>,
    #[clap(long)]
    checkpoint_hash: Option<String>,
}

impl Cli {
    pub fn to_config(self) -> Config {
        let chain = match self.network.as_str() {
            "optimism-goerli" => ChainConfig::optimism_goerli(),
            "base-goerli" => ChainConfig::base_goerli(),
            _ => panic!("network not recognized"),
        };

        let config_path = home_dir().unwrap().join(".magi/magi.toml");
        let cli_config = CliConfig::from(self);
        Config::new(&config_path, cli_config, chain)
    }
}

impl From<Cli> for CliConfig {
    fn from(value: Cli) -> Self {
        Self {
            l1_rpc_url: value.l1_rpc_url,
            l2_rpc_url: value.l2_rpc_url,
            l2_engine_url: value.l2_engine_url,
            jwt_secret: value.jwt_secret,
        }
    }
}

fn shutdown_on_ctrlc(shutdown_sender: Sender<bool>) {
    ctrlc::set_handler(move || {
        tracing::info!(target: "magi", "shutting down");
        shutdown_sender.send(true).expect("shutdown failure");
    })
    .expect("could not register shutdown handler");
}
