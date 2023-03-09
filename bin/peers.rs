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
    telemetry::init(false)?;
    telemetry::register_shutdown();

    let cli = Cli::parse();
    let config = cli.to_config();

    // TODO: peer discovery

    Ok(())
}

/// The CLI for magi peering
#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[clap(
        short = 'v',
        long,
        default_value = "info",
        help = "Sets the logging verbosity level."
    )]
    pub log_level: logging::LogLevel,

    #[clap(subcommand)]
    pub subcommand: Option<Subcommand>,
}

/// Discv5-cli Subcommand
#[derive(ClapSubcommand, Clone, Debug)]
#[allow(missing_docs)]
pub enum Subcommand {
    #[clap(name = "packet", about = "Performs packet operations")]
    Packet(crate::packet::Packet),
    #[clap(name = "request-enr", about = "Requests an ENR from a node")]
    RequestEnr(crate::request_enr::RequestEnr),
    #[clap(name = "server", about = "Runs a discv5 test server")]
    Server(crate::server::Server),
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
