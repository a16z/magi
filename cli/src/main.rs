use std::{
    process::exit,
    sync::{Arc, Mutex},
    str::FromStr,
};

use clap::Parser;
use dirs::home_dir;
use ethers_core::types::Address;
use eyre::Result;

use log::{error, info};

use magi::{
    common::BlockID,
    config::{ChainConfig, Config, SyncMode},
    derive::Pipeline,
    driver::Driver,
    engine::{EngineApi},
    telemetry,
};

#[tokio::main]
async fn main() -> Result<()> {
    telemetry::init(false)?;

    let config = get_config();

    let pipeline = Pipeline::new(config.chain.l1_start_epoch.number, config.clone().into())?;
    let engine = EngineApi::new(config.get_engine_api_url(), config.jwt_secret.clone());
    let mut driver = Driver::new(engine, pipeline, config.into());
    if let Err(err) = driver.start().await {
        error!("{}", err);
        exit(1);
    }

    // let a_driver = Arc::new(driver);
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
            exit(0);
            // let a_driver = a_driver.clone();
            // std::thread::spawn(move || {
            //     block_on(a_driver.shutdown());
            //     exit(0);
            // });
        }
    })
    .expect("could not register shutdown handler");

    std::future::pending().await
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
    #[clap(short = 't', long)]
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
    #[clap(short, long)]
    engine_api_url: Option<String>,
    #[clap(short, long)]
    jwt_secret: Option<String>,
}

impl Cli {
    fn to_config(&self) -> Config {
        Config {
            l1_rpc_url: self.l1_rpc_url.clone(),
            l2_rpc_url: Some(self.l2_rpc_url.clone()),
            chain: ChainConfig {
                l1_start_epoch: BlockID::from_str(&self.l1_start_epoch).unwrap(),
                l2_genesis: BlockID::from_str(&self.l2_genesis).unwrap(),
                batch_sender: Address::from_str(&self.batch_sender).unwrap(),
                batch_inbox: Address::from_str(&self.batch_inbox).unwrap(),
                deposit_contract: Address::from_str(&self.deposit_contract).unwrap(),
            },
            max_channels: self.max_channels as usize,
            max_timeout: self.max_timeout,
            engine_api_url: self.engine_api_url.clone(),
            jwt_secret: self.jwt_secret.clone(),
        }
    }
}