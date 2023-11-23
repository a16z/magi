use std::{fs, net::SocketAddr, path::PathBuf, process};

use clap::Parser;
use dirs::home_dir;
use discv5::enr::{CombinedKey, Enr};
use eyre::{anyhow, Result};
use libp2p_identity::secp256k1::SecretKey;
use serde::Serialize;

use magi::{
    config::{
        secret_key_from_hex, serialize_secret_key, ChainConfig, CliConfig, Config, ConfigBuilder,
        SyncMode,
    },
    network,
    runner::Runner,
    telemetry::{self, metrics},
};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let sync_mode = cli.sync_mode.clone();
    let verbose = cli.verbose;
    let logs_dir = cli.logs_dir.clone();
    let logs_rotation = cli.logs_rotation.clone();
    let checkpoint_hash = cli.checkpoint_hash.clone();
    let metrics_listen = cli.metrics_listen;
    let config = cli.to_config()?;

    let _guards = telemetry::init(verbose, logs_dir, logs_rotation);
    metrics::init(metrics_listen)?;

    let runner = Runner::from_config(config)
        .with_sync_mode(sync_mode)
        .with_checkpoint_hash(checkpoint_hash);

    if let Err(err) = runner.run().await {
        tracing::error!(target: "magi", "{}", err);
        process::exit(1);
    }

    Ok(())
}

#[derive(Debug, Parser, Serialize)]
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
    #[clap(long = "sequencer-enabled")]
    sequencer_enabled: bool,
    #[clap(long = "sequencer-max-safe-lag", default_value = "0")]
    sequencer_max_safe_lag: String,

    /// P2P listening address
    #[clap(long, default_value = network::LISTENING_AS_STR)]
    p2p_listen: SocketAddr,

    /// Secret key Secp256k1 for P2P.
    /// You can pass both the path to the key and the value of the private key itself
    /// The private key must be in hexadecimal format with a length of 64 characters.
    /// Example:
    ///     fe438c63458706e03479442743baae6c88256498e6431708f6dfc520a26515d3
    ///     /path/to/secret_key
    #[clap(
        long,
        value_parser = parse_secret_key_from_cli,
        verbatim_doc_comment
    )]
    #[serde(
        serialize_with = "serialize_secret_key",
        skip_serializing_if = "Option::is_none"
    )]
    p2p_secret_key: Option<SecretKey>,

    /// Bootnodes to which you need to connect initially. A list of addresses separated by a space is expected in ENR format
    ///     
    /// If not specified, the optimism mainnet will be used.
    ///
    /// Example:
    ///     enr:<BASE_64>_1 enr:<BASE_64>_2 ... enr:<BASE_64>_N
    #[clap(
        long,
        verbatim_doc_comment,
        value_delimiter = ' ',
        num_args = 1..
    )]
    p2p_bootnodes: Option<Vec<Enr<CombinedKey>>>,

    /// Secret key Secp256k1 for Sequencer.
    /// You can pass both the path to the key and the value of the private key itself
    /// The private key must be in hexadecimal format with a length of 64 characters.
    /// Example:
    ///     fe438c63458706e03479442743baae6c88256498e6431708f6dfc520a26515d3
    ///     /path/to/secret_key
    #[clap(
        long,
        value_parser = parse_secret_key_from_cli,
        verbatim_doc_comment
    )]
    #[serde(
        serialize_with = "serialize_secret_key",
        skip_serializing_if = "Option::is_none"
    )]
    p2p_sequencer_secret_key: Option<SecretKey>,

    /// Metrics listening address.
    /// The parameter wouldn't be saved as part as config.
    #[clap(long, default_value = metrics::LISTENING_AS_STR)]
    metrics_listen: SocketAddr,

    /// Specify the magi working directory. It will store all the necessary data for the launch.
    #[clap(long, short = 'd', verbatim_doc_comment, default_value = default_working_dir())]
    #[serde(skip)]
    working_dir: PathBuf,

    /// Save the configuration to launch Magi in the future
    /// The configuration will be saved in the working directory named "magi.toml": <WORK_DIR>/magi.toml
    #[clap(long = "save", short = 's', verbatim_doc_comment)]
    #[serde(skip)]
    save_config: bool,
}

impl Cli {
    pub fn to_config(self) -> eyre::Result<Config> {
        let chain = ChainConfig::try_from(self.network.as_str())?;

        let mut work_dir = self.working_dir.clone();
        if !work_dir.is_absolute() {
            work_dir = std::env::current_dir()?.join(work_dir);
        }

        let magi_config_path = work_dir.join("magi.toml");
        let save = self.save_config;
        let cli_config = CliConfig::try_from(self)?;

        let config = ConfigBuilder::default()
            .chain(chain)
            .toml(&magi_config_path)
            .cli(cli_config)
            .build();

        if save {
            config.save(magi_config_path)?;
        }

        Ok(config)
    }
}

impl TryFrom<Cli> for CliConfig {
    type Error = eyre::Report;

    fn try_from(value: Cli) -> Result<Self> {
        let sequencer = match value.sequencer_enabled {
            true => Some(value.sequencer_max_safe_lag.try_into()?),
            false => None,
        };

        Ok(Self {
            l1_rpc_url: value.l1_rpc_url,
            l2_rpc_url: value.l2_rpc_url,
            l2_engine_url: value.l2_engine_url,
            jwt_secret: value.jwt_secret,
            checkpoint_sync_url: value.checkpoint_sync_url,
            rpc_port: value.rpc_port,
            devnet: value.devnet,
            sequencer,
            p2p_secret_key: value.p2p_secret_key,
            p2p_listen: value.p2p_listen,
            p2p_bootnodes: value.p2p_bootnodes,
            p2p_sequencer_secret_key: value.p2p_sequencer_secret_key,
        })
    }
}

/// The incoming value is the path to the key or a string with the key.
/// The private key must be in hexadecimal format with a length of 64 characters.
fn parse_secret_key_from_cli(value: &str) -> Result<SecretKey> {
    secret_key_from_hex(value).or_else(|err| {
        let path = PathBuf::try_from(value).map_err(|_| err)?;
        let key_string = fs::read_to_string(&path)
            .map_err(|_| anyhow!("The key file {path:?} was not found."))?
            .trim()
            .to_string();

        let key = secret_key_from_hex(&key_string)?;

        Ok(key)
    })
}

fn default_working_dir() -> String {
    home_dir()
        .expect(
            "Could not determine the home directory in the operating system. \
            Specify the working directory using the \"--work-dir\" parameter",
        )
        .join(".magi/")
        .display()
        .to_string()
}
