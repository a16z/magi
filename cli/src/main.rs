use std::{
    sync::{Arc, Mutex},
    str::FromStr,
    path::PathBuf,
};

use clap::Parser;
use dirs::home_dir;
use ethers_core::types::{H256, H160, Chain};
use eyre::Result;

use magi::{
    l1::ChainWatcher,
    common::{BlockInfo, Epoch},
    config::{ChainConfig, Config, SyncMode},
    derive::{Pipeline, state::State},
    driver::Driver,
    engine::{EngineApi},
    telemetry,
};

#[tokio::main]
async fn main() -> Result<()> {
    telemetry::init(false)?;
    telemetry::register_shutdown();

    // Construct all magi components
    let cli = Cli::parse();
    let sync_mode = cli.sync_mode.clone();
    let config = cli.to_config();
    println!("Config: {:#?}", config);

    // TODO: if we spawn slow sync in a separate thread that writes to db,
    // TODO: and fast sync writes an invalid payload to db, we need to bubble
    // TODO: this error up to the slow sync thread and the main thread.

    // We want to spawn slow sync in a separate thread
    // this allows the happy-path to gracefully fail without
    // delaying slow sync.
    let arc_config = Arc::new(config);
    slow_sync(arc_config).await?;
    // let slow_sync = std::thread::spawn(|| async move {
    //     slow_sync(arc_config).await
    // });

    // If we have fast sync enabled, we need to sync the state first
    if sync_mode == SyncMode::Fast {
        tracing::info!(target: "magi", "syncing in fast mode...");
        // TODO:: fast sync
        panic!("fast sync not implemented yet");
        // driver.fast_sync().await?;
    } else {
        tracing::info!(target: "magi", "syncing in challenge or full mode...");
        // let res = slow_sync.join();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        // println!("slow sync finished: {:?}", res);
        // loop join until slow sync is finished
        // loop {
        //     // TODO: add an optional timeout for slow sync
        //     if slow_sync.is_finished() {
        //         tracing::info!(target: "magi", "slow sync finished");
        //         break;
        //     }
        // }
    }
}

pub async fn slow_sync(config: Arc<Config>) -> Result<()> {
    let chain_number = config.chain.l1_start_epoch.number;
    let mut chain_watcher = ChainWatcher::new(chain_number, config.clone()).unwrap();
    let tx_recv = chain_watcher.take_tx_receiver().unwrap();
    let state = Arc::new(Mutex::new(State::new(
        config.chain.l2_genesis,
        config.chain.l1_start_epoch,
        chain_watcher,
    )));
    tracing::info!(target: "magi", "starting full sync on chain {}", chain_number);
    let pipeline = match Pipeline::new(state, tx_recv, config.clone().into()) {
        Ok(pipeline) => pipeline,
        Err(err) => {
            tracing::error!(target: "magi", "Pipeline construction error: {}", err);
            std::process::exit(1);
        }
    };
    tracing::info!(target: "magi", "created pipeline");
    let engine = EngineApi::new(config.get_engine_api_url(), config.jwt_secret.clone());
    tracing::info!(target: "magi", "constructed engine");
    let driver = Driver::from_internals(engine, pipeline, Arc::clone(&config));
    tracing::info!(target: "magi", "executing driver...");

    // Run the driver
    if let Err(err) = driver.unwrap().start().await {
        tracing::error!(target: "magi", "{}", err);
        std::process::exit(1);
    }

    Ok(())
}

#[derive(Parser)]
pub struct Cli {
    #[clap(short, long, default_value = "goerli")]
    network: Chain,
    #[clap(long, default_value = "~/.magi/db")]
    db_location: String,
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
    #[clap(long, default_value = "300")]
    max_seq_drif: u64,
    #[clap(long, default_value = "120")]
    seq_window_size: u64,
    #[clap(short = 'm', long, default_value = "fast")]
    sync_mode: SyncMode,
    #[clap(short = 'e', long)]
    engine_api_url: Option<String>,
    #[clap(short = 'j', long)]
    jwt_secret: Option<String>,
}

impl Cli {
    pub fn to_config(self) -> Config {
        let config_path = home_dir().unwrap().join(".magi/magi.toml");
        let cli_config: Config = self.cli_config();
        let base_config = Config::from_file(&config_path, &cli_config);
        let local_path = std::env::current_dir().unwrap().join("magi.toml");
        Config::from_file(&local_path, &base_config)
    }

    pub fn cli_config(&self) -> Config {
        let l1_start_epoch = self.l1_start_epoch.clone().unwrap_or("".to_string());
        let l2_genesis = self.l2_genesis.clone().unwrap_or("".to_string());
        Config {
            l1_rpc_url: self.l1_rpc_url.clone(),
            l2_rpc_url: self.l2_rpc_url.clone(),
            chain: ChainConfig {
                l1_start_epoch: Epoch {
                    number: 0,
                    hash: H256::from_str(&l1_start_epoch).unwrap_or_default(),
                    timestamp: 0,
                },
                l2_genesis: BlockInfo {
                    hash: H256::from_str(&l2_genesis).unwrap_or_default(),
                    number: 0,
                    parent_hash: H256::zero(),
                    timestamp: 0,
                },
                batch_sender: parsed_or_zero(&self.batch_sender),
                batch_inbox: parsed_or_zero(&self.batch_inbox),
                deposit_contract: parsed_or_zero(&self.deposit_contract),
                max_channels: self.max_channels as usize,
                max_timeout: self.max_timeout,
                max_seq_drif: self.max_seq_drif,
                seq_window_size: self.seq_window_size,
            },
            db_location: PathBuf::from_str(&self.db_location).ok(),
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