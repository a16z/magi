use std::{
    path::PathBuf,
    process::exit,
    str::FromStr,
    sync::{Arc, Mutex},
};

use clap::Parser;
use dirs::home_dir;
use env_logger::Env;
use eyre::Result;

use client::{database::FileDB, Client, ClientBuilder};
use config::{CliConfig, Config};
use futures::executor::block_on;
use log::{error, info};

use magi::{
    config::{ChainConfig, Config},
    derive::Pipeline,
    driver::Driver,
    engine::EngineApi,
    telemetry,
};

#[tokio::main]
async fn main() -> Result<()> {
    telemetry::init(false)?;

    let config = get_config();

    let pipeline = Pipeline::new(config.chain.l1_start_epoch.number, config.clone())?;
    let engine = EngineApi::new(config.get_engine_api_url(), config.jwt_secret);
    let mut driver = Driver::new(engine, pipeline, config);
    if let Err(err) = driver.start().await {
        error!("{}", err);
        exit(1);
    }

    register_shutdown_handler(driver);
    std::future::pending().await
}

fn register_shutdown_handler(driver: Driver) {
    let client = Arc::new(driver);
    let shutdown_counter = Arc::new(Mutex::new(0));

    ctrlc::set_handler(move || {
        let mut counter = shutdown_counter.lock().unwrap();
        *counter += 1;

        let counter_value = *counter;

        if counter_value == 3 {
            info!("forced shutdown");
            exit(0);
        }

        info!(
            "shutting down... press ctrl-c {} more times to force quit",
            3 - counter_value
        );

        if counter_value == 1 {
            let client = client.clone();
            std::thread::spawn(move || {
                block_on(client.shutdown());
                exit(0);
            });
        }
    })
    .expect("could not register shutdown handler");
}

fn get_config() -> Config {
    let cli = Cli::parse();
    let config_path = home_dir().unwrap().join(".helios/helios.toml");
    let cli_config: Config = cli.to_config();
    Config::from_file(&config_path, &cli_config)
}

#[derive(Parser)]
struct Cli {
    #[clap(short, long)]
    l1_rpc_url: String,
    #[clap(short = "t", long)]
    l2_rpc_url: String,
    #[clap(short, long)]
    l1_start_epoch: String,
    #[clap(short, long)]
    l2_genesis: String,
    #[clap(short, long)]
    batch_sender: String,
    #[clap(short, long)]
    batch_inbox: String,
    #[clap(short, long)]
    deposit_contract: String,
    #[clap(short, long)]
    max_channels: u64,
    #[clap(short, long)]
    max_timeout: u64,
    #[clap(short, long)]
    sync_mode: SyncMode,
}

impl Cli {
    fn to_config(&self) -> Config {
        Config {
            l1_rpc: self.l1_rpc_url.clone(),
            l2_rpc: Some(self.l2_rpc_url.clone()),
            chain: ChainConfig {
                l1_start_epoch: Epoch {
                    number: self.l1_start_epoch.parse().unwrap(),
                    block_hash: None,
                },
                l2_genesis: self.l2_genesis.clone(),
                batch_sender: self.batch_sender.clone(),
                batch_inbox: self.batch_inbox.clone(),
                deposit_contract: self.deposit_contract.clone(),
            },
            max_channels: self.max_channels,
            max_timeout: self.max_timeout,
        }
    }
}