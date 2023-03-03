use std::{
    process::exit,
    sync::{Arc, Mutex},
    str::FromStr,
};

use clap::Parser;
use dirs::home_dir;
use ethers_core::types::{H160, Chain};
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
            // std::thread::spawn(move || {
            //     block_on(driver.shutdown());
            //     exit(0);
            // });
        }
    })
    .expect("could not register shutdown handler");

    std::future::pending().await
}

fn get_config() -> Config {
    let cli = Cli::parse();
    let config_path = home_dir().unwrap().join(".magi/magi.toml");
    let cli_config: Config = cli.to_config();
    let base_config = Config::from_file(&config_path, &cli_config);
    let local_path = std::env::current_dir().unwrap().join("magi.toml");
    Config::from_file(&local_path, &base_config)
}

#[derive(Parser)]
pub struct Cli {
    #[clap(short, long, default_value = "goerli")]
    network: Chain,
    #[clap(long, default_value = "")]
    l1_rpc_url: String,
    #[clap(long)]
    l2_rpc_url: Option<String>,
    #[clap(long)]
    l1_start_epoch: Option<String>,
    #[clap(long)]
    l2_genesis: Option<String>,
    #[clap(long)]
    batch_sender: Option<String>,
    #[clap(long)]
    batch_inbox: Option<String>,
    #[clap(long)]
    deposit_contract: Option<String>,
    #[clap(long, default_value = "100000000")]
    max_channels: u64,
    #[clap(long, default_value = "100")]
    max_timeout: u64,
    #[clap(short = 'm', long, default_value = "fast")]
    sync_mode: SyncMode,
    #[clap(short = 'e', long)]
    engine_api_url: Option<String>,
    #[clap(short = 'j', long)]
    jwt_secret: Option<String>,
}

impl Cli {
    fn to_config(&self) -> Config {
        Config {
            l1_rpc_url: self.l1_rpc_url.clone(),
            l2_rpc_url: self.l2_rpc_url.clone(),
            chain: ChainConfig {
                l1_start_epoch: BlockID::from_str(&self.l1_start_epoch.clone().unwrap_or("".to_string())).unwrap(),
                l2_genesis: BlockID::from_str(&self.l2_genesis.clone().unwrap_or("".to_string())).unwrap(),
                batch_sender: parsed_or_zero(&self.batch_sender),
                batch_inbox: parsed_or_zero(&self.batch_inbox),
                deposit_contract: parsed_or_zero(&self.deposit_contract),
            },
            max_channels: self.max_channels as usize,
            max_timeout: self.max_timeout,
            engine_api_url: self.engine_api_url.clone(),
            jwt_secret: self.jwt_secret.clone(),
        }
    }
}

pub fn parsed_or_zero(s: &Option<String>) -> H160 {
    match s {
        Some(s) => match H160::from_str(s) {
            Ok(v) => v,
            Err(_) => H160::zero(),
        },
        None => H160::zero(),
    }
}