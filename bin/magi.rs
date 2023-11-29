use std::path::PathBuf;
use std::{env::current_dir, process};

use clap::Parser;
use dirs::home_dir;
use eyre::Result;

use magi::config::LocalSequencerConfig;
use magi::{
    config::{ChainConfig, CliConfig, Config, SyncMode},
    runner::Runner,
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

    let runner = Runner::from_config(config)
        .with_sync_mode(sync_mode)
        .with_checkpoint_hash(checkpoint_hash);

    if let Err(err) = runner.run().await {
        tracing::error!(target: "magi", "{}", err);
        process::exit(1);
    }

    Ok(())
}

#[derive(Parser, Serialize)]
pub struct Cli {
    #[clap(short, long, default_value = "optimism")]
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
    /// Path to a JWT secret to use for authenticated RPC endpoints
    #[clap(long)]
    jwt_file: Option<PathBuf>,
    #[clap(short = 'v', long)]
    verbose: bool,
    #[clap(short = 'p', long)]
    rpc_port: Option<u16>,
    #[clap(long)]
    logs_dir: Option<String>,
    #[clap(long)]
    logs_rotation: Option<String>,
    #[clap(long)]
    checkpoint_hash: Option<String>,
    #[clap(long)]
    checkpoint_sync_url: Option<String>,
    #[clap(long)]
    devnet: bool,
    #[clap(flatten)]
    local_sequencer: LocalSequencerCli,
}

#[derive(Parser, Serialize)]
pub struct LocalSequencerCli {
    #[clap(long = "sequencer")]
    enabled: bool,
    #[clap(long = "sequencer-max-safe-lag", default_value = "0")]
    max_safe_lag: u64,
    #[clap(long = "sequencer-pk-file")]
    pk_file: Option<PathBuf>,
}

impl Cli {
    pub fn to_config(self) -> Config {
        let chain = match self.network.as_str() {
            "optimism" => ChainConfig::optimism(),
            "optimism-goerli" => ChainConfig::optimism_goerli(),
            "optimism-sepolia" => ChainConfig::optimism_sepolia(),
            "base" => ChainConfig::base(),
            "base-goerli" => ChainConfig::base_goerli(),
            path if ChainConfig::is_specular_config(path) => ChainConfig::from_specular_json(path),
            file if file.ends_with(".json") => ChainConfig::from_json(file),
            _ => panic!(
                "Invalid network name. \\
                Please use one of the following: 'optimism', 'optimism-goerli', 'base-goerli'. \\
                You can also use a JSON file path for custom configuration."
            ),
        };

        let config_path = home_dir().unwrap().join(".magi/magi.toml");
        let cli_config = CliConfig::from(self);
        Config::new(&config_path, cli_config, chain)
    }

    pub fn jwt_secret(&self) -> Option<String> {
        self.jwt_secret.clone().or(self.jwt_secret_from_file())
    }

    pub fn jwt_secret_from_file(&self) -> Option<String> {
        let jwt_file = self.jwt_file.as_ref()?;
        match std::fs::read_to_string(jwt_file) {
            Ok(content) => Some(content),
            Err(_) => Cli::default_jwt_secret(),
        }
    }

    pub fn default_jwt_secret() -> Option<String> {
        let cur_dir = current_dir().ok()?;
        match std::fs::read_to_string(cur_dir.join("jwt.hex")) {
            Ok(content) => Some(content),
            Err(_) => {
                tracing::error!(target: "magi", "Failed to read JWT secret from file: {:?}", cur_dir);
                None
            }
        }
    }
}

impl From<Cli> for CliConfig {
    fn from(value: Cli) -> Self {
        let jwt_secret = value.jwt_secret();
        Self {
            l1_rpc_url: value.l1_rpc_url,
            l2_rpc_url: value.l2_rpc_url,
            l2_engine_url: value.l2_engine_url,
            jwt_secret,
            checkpoint_sync_url: value.checkpoint_sync_url,
            rpc_port: value.rpc_port,
            devnet: value.devnet,
            local_sequencer: Some(value.local_sequencer.into()),
        }
    }
}

impl LocalSequencerCli {
    pub fn read_private_key(&self) -> Option<String> {
        let pk_file = self.pk_file.as_ref()?;
        match std::fs::read_to_string(pk_file) {
            Ok(content) => Some(content),
            Err(_) => {
                tracing::error!(target: "magi", "Failed to read sequencer pk from file: {:?}", pk_file);
                None
            }
        }
    }
}

impl From<LocalSequencerCli> for LocalSequencerConfig {
    fn from(value: LocalSequencerCli) -> Self {
        Self {
            enabled: value.enabled,
            max_safe_lag: value.max_safe_lag,
            private_key: value.read_private_key(),
        }
    }
}
