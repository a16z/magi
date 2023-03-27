use std::{collections::HashMap, sync::mpsc::channel};

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
    let cli = Cli::parse();
    let sync_mode = cli.sync_mode.clone();
    let verbose = cli.verbose;
    let config = cli.to_config();

    telemetry::init(verbose)?;

    match sync_mode {
        SyncMode::Fast => panic!("fast sync not implemented"),
        SyncMode::Full => full_sync(config).await?,
        SyncMode::Challenge => panic!("challenge sync not implemented"),
    };

    Ok(())
}

pub async fn full_sync(config: Config) -> Result<()> {
    tracing::info!(target: "magi", "starting full sync");
    let (shutdown_sender, shutdown_recv) = channel();

    let mut driver = Driver::from_config(config, shutdown_recv)?;

    ctrlc::set_handler(move || {
        tracing::info!(target: "magi", "shutting down");
        shutdown_sender.send(true).expect("shutdown failure");
    })
    .expect("could not register shutdown handler");

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
    l2_engine_url: Option<String>,
    #[clap(short = 'j', long)]
    jwt_secret: Option<String>,
    #[clap(short = 'v', long)]
    verbose: bool,
}

impl Cli {
    pub fn to_config(self) -> Config {
        let chain = match self.network.as_str() {
            "optimism-goerli" => ChainConfig::optimism_goerli(),
            "base-goerli" => ChainConfig::base_goerli(),
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

        if let Some(l2_engine_url) = &self.l2_engine_url {
            user_dict.insert("l2_engine_url", Value::from(l2_engine_url.clone()));
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
