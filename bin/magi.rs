use std::{
    sync::{Arc, Mutex},
    str::FromStr,
    path::PathBuf,
    process::exit,
};

use clap::Parser;
use dirs::home_dir;
use eyre::Result;
use tracing::info;

use magi::{
    config::{ChainConfig, Config, SyncMode},
    driver::Driver,
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

    // TODO: if we spawn slow sync in a separate thread that writes to db,
    // TODO: and fast sync writes an invalid payload to db, we need to bubble
    // TODO: this error up to the slow sync thread and the main thread.

    // We want to spawn slow sync in a separate thread
    // this allows the happy-path to gracefully fail without
    // delaying slow sync.
    full_sync(config).await?;
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

pub async fn full_sync(config: Config) -> Result<()> {
    let chain_number = config.chain.l1_start_epoch.number;
    tracing::info!(target: "magi", "starting full sync on chain {}", chain_number);
    let mut driver = Driver::from_config(config);
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
    #[clap(short, long, default_value = "optimism-goerli")]
    network: String,
    #[clap(long)]
    db_location: Option<String>,
    #[clap(long, default_value = "")]
    l1_rpc_url: String,
    #[clap(long)]
    l2_rpc_url: Option<String>,
    #[clap(short = 'm', long, default_value = "fast")]
    sync_mode: SyncMode,
    #[clap(short = 'e', long)]
    engine_api_url: Option<String>,
    #[clap(short = 'j', long)]
    jwt_secret: Option<String>,
}

impl Cli {
    pub fn to_config(self) -> Config {
        let chain = match self.network.as_str() {
            "optimism-goerli" => ChainConfig::goerli(),
            _ => panic!("network not recognized"),
        };

        let default_db_loc = home_dir().unwrap().join(".magi/data");
        let db_loc = self.db_location.map(|f| PathBuf::from_str(&f).ok()).flatten().unwrap_or(default_db_loc);

        Config {
            l1_rpc_url: self.l1_rpc_url.clone(),
            l2_rpc_url: self.l2_rpc_url.clone(),
            db_location: Some(db_loc),
            engine_api_url: self.engine_api_url.clone(),
            jwt_secret: self.jwt_secret.clone(),
            chain,
        }
    }
}

fn register_shutdown_handler() {
    let shutdown_counter = Arc::new(Mutex::new(0));

    ctrlc::set_handler(move || {
        let mut counter = shutdown_counter.lock().unwrap();
        *counter += 1;

        let counter_value = *counter;

        if counter_value == 1 {
            // TODO: handle driver shutdown
            exit(0);
        }

        if counter_value == 3 {
            info!("forced shutdown");
            exit(0);
        }

        info!(
            "shutting down... press ctrl-c {} more times to force quit",
            3 - counter_value
        );
    })
    .expect("could not register shutdown handler");
}

