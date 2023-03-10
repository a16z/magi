use std::collections::HashMap;

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
    telemetry::init(false, None)?;
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
    // full_sync(config).await?;

    // let slow_sync = std::thread::spawn(|| async move {
    //     slow_sync(arc_config).await
    // });

    match sync_mode {
        SyncMode::Fast => panic!("fast sync not implemented"),
        SyncMode::Full => full_sync(config).await?,
        SyncMode::Challenge => panic!("challenge sync not implemented"),
    };

    Ok(())
}

pub async fn full_sync(config: Config) -> Result<()> {
    tracing::info!(target: "magi", "starting full sync");
    let mut driver = Driver::from_config(config)?;

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
    #[clap(long, default_value_t = default_data_dir())]
    data_dir: String,
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
        Config::new(&config_path, self.as_provider(), chain)
    }

    pub fn as_provider(&self) -> Serialized<HashMap<&str, Value>> {
        let mut user_dict = HashMap::new();

        user_dict.insert("data_dir", Value::from(self.data_dir.clone()));

        if let Some(l1_rpc) = &self.l1_rpc_url {
            user_dict.insert("l1_rpc_url", Value::from(l1_rpc.clone()));
        }

        if let Some(l2_rpc) = &self.l2_rpc_url {
            user_dict.insert("l2_rpc_url", Value::from(l2_rpc.clone()));
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

fn default_data_dir() -> String {
    let dir = home_dir().unwrap().join(".magi/data");
    dir.to_str().unwrap().to_string()
}
