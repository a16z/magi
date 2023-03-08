use std::{collections::HashMap, path::PathBuf, str::FromStr};

use clap::Parser;
use dirs::home_dir;
use eyre::Result;
use figment::{providers::Serialized, value::Value};

use magi::{
    config::{ChainConfig, Config, SyncMode},
    driver::Driver,
    telemetry,
};

#[tokio::main]
async fn main() -> Result<()> {
    telemetry::init(false)?;
    telemetry::register_shutdown();

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
    tracing::info!(target: "magi", "starting full sync");
    let mut driver = Driver::from_config(config)?;
    tracing::info!(target: "magi", "executing driver...");

    // Run the driver
    if let Err(err) = driver.start().await {
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
    #[clap(long)]
    l1_rpc_url: Option<String>,
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

        let config_path = home_dir().unwrap().join(".magi/magi.toml");
        let mut config = Config::new(&config_path, self.as_provider(), chain);

        let default_db_loc = home_dir().unwrap().join(".magi/data");
        let db_loc = self
            .db_location
            .and_then(|f| PathBuf::from_str(&f).ok())
            .unwrap_or(default_db_loc);

        config.db_location = Some(db_loc);

        config
    }

    pub fn as_provider(&self) -> Serialized<HashMap<&str, Value>> {
        let mut user_dict = HashMap::new();

        if let Some(l1_rpc) = &self.l1_rpc_url {
            user_dict.insert("l1_rpc_url", Value::from(l1_rpc.clone()));
        }

        if let Some(l2_rpc) = &self.l2_rpc_url {
            user_dict.insert("l2_rpc_url", Value::from(l2_rpc.clone()));
        }

        if let Some(db_loc) = &self.db_location {
            user_dict.insert("db_location", Value::from(db_loc.clone()));
        }

        if let Some(engine_api_url) = &self.engine_api_url {
            user_dict.insert("engine_api_url", Value::from(engine_api_url.clone()));
        }

        if let Some(jwt_secret) = &self.jwt_secret {
            user_dict.insert("jwt_secret", Value::from(jwt_secret.clone()));
        }

        Serialized::from(user_dict, "default".to_string())
    }
}
