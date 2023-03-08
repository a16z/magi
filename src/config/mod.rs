use std::{collections::HashMap, path::PathBuf, process::exit, str::FromStr};

use ethers_core::types::{Address, H256};
use figment::{
    providers::{Data, Format, Serialized, Toml},
    value::{Dict, Tag, Value},
    Error, Figment,
};
use serde::{Deserialize, Serialize};

use crate::common::{BlockInfo, Epoch};

/// Sync Mode Specifies how `magi` should sync the L2 chain
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SyncMode {
    /// Fast sync mode
    Fast,
    /// Challenge sync mode
    Challenge,
    /// Full sync mode
    Full,
}

impl FromStr for SyncMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "fast" => Ok(Self::Fast),
            "challenge" => Ok(Self::Challenge),
            "full" => Ok(Self::Full),
            _ => Err("invalid sync mode".to_string()),
        }
    }
}

/// A system configuration
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// The base chain RPC URL
    pub l1_rpc_url: String,
    /// The L2 engine RPC URL
    pub l2_rpc_url: Option<String>,
    /// Engine API URL
    pub engine_api_url: Option<String>,
    /// The base chain config
    pub chain: ChainConfig,
    /// Location of the database folder
    pub db_location: Option<PathBuf>,
    /// Engine API JWT Secret
    /// This is used to authenticate with the engine API
    pub jwt_secret: Option<String>,
}

impl Config {
    pub fn get_engine_api_url(&self) -> String {
        self.engine_api_url
            .clone()
            .unwrap_or("http://localhost:8551".to_string())
    }
}

impl Config {
    pub fn from_file(config_path: &PathBuf, cli_config: &Config) -> Self {
        let toml_provider: Data<Toml> = Toml::file(config_path).nested();
        let cli_provider = cli_config.as_provider();
        let config_res: Result<Config, Error> = Figment::new()
            .merge(cli_provider)
            .merge(toml_provider)
            .extract();
        match config_res {
            Ok(config) => {
                // if config.chain.is_zero
                config
            }
            Err(err) => {
                match err.kind {
                    figment::error::Kind::MissingField(field) => {
                        let field = field.replace('_', "-");
                        println!("\x1b[91merror\x1b[0m: missing configuration field: {field}");
                        println!("\n\ttry supplying the propoper command line argument: --{field}");
                        println!("\talternatively, you can add the field to your magi.toml file or as an environment variable");
                        println!("\nfor more information, check the github README");
                    }
                    _ => println!("cannot parse configuration: {err}"),
                }
                exit(1);
            }
        }
    }

    pub fn as_provider(&self) -> Serialized<HashMap<&str, Value>> {
        let mut user_dict = HashMap::new();
        user_dict.insert("l1_rpc_url", Value::from(self.l1_rpc_url.clone()));
        if let Some(l2_rpc) = &self.l2_rpc_url {
            user_dict.insert("l2_rpc_url", Value::from(l2_rpc.clone()));
        }
        if let Some(db_loc) = &self.db_location {
            if let Some(str) = db_loc.to_str() {
                user_dict.insert("db_location", Value::from(str));
            }
        }
        if let Some(engine_api_url) = &self.engine_api_url {
            user_dict.insert("engine_api_url", Value::from(engine_api_url.clone()));
        }
        if let Some(jwt_secret) = &self.jwt_secret {
            user_dict.insert("jwt_secret", Value::from(jwt_secret.clone()));
        }
        user_dict.insert("chain", Value::from(self.chain));
        Serialized::from(user_dict, "default".to_string())
    }
}

/// A Chain Configuration
#[derive(Debug, Copy, Clone, Deserialize)]
pub struct ChainConfig {
    /// The L1 block referenced by the L2 chain
    pub l1_start_epoch: Epoch,
    /// The L2 genesis block info
    pub l2_genesis: BlockInfo,
    /// The batch sender address
    pub batch_sender: Address,
    /// The batch inbox address
    pub batch_inbox: Address,
    /// The deposit contract address
    pub deposit_contract: Address,
    /// The maximum number of intermediate pending channels
    pub max_channels: usize,
    /// The max timeout for a channel (as measured by the frame L1 block number)
    pub max_timeout: u64,
    /// Number of L1 blocks in a sequence window
    pub seq_window_size: u64,
    /// Maximum timestamp drift
    pub max_seq_drif: u64,
}

fn address_to_str(address: &Address) -> String {
    format!("0x{}", hex::encode(address.as_bytes()))
}

impl From<ChainConfig> for Value {
    fn from(value: ChainConfig) -> Value {
        let mut dict = Dict::new();
        dict.insert(
            "l1_start_epoch".to_string(),
            Value::from(value.l1_start_epoch),
        );
        dict.insert("l2_genesis".to_string(), Value::from(value.l2_genesis));
        dict.insert(
            "batch_sender".to_string(),
            Value::from(address_to_str(&value.batch_sender)),
        );
        dict.insert(
            "batch_inbox".to_string(),
            Value::from(address_to_str(&value.batch_inbox)),
        );
        dict.insert(
            "deposit_contract".to_string(),
            Value::from(address_to_str(&value.deposit_contract)),
        );
        dict.insert("max_channels".to_string(), Value::from(value.max_channels));
        dict.insert("max_timeout".to_string(), Value::from(value.max_timeout));
        Value::Dict(Tag::Default, dict)
    }
}

/// System accounts
#[derive(Debug, Clone)]
pub struct SystemAccounts {
    pub attributes_depositor: Address,
    pub attributes_predeploy: Address,
    pub fee_vault: Address,
}

impl ChainConfig {
    pub fn goerli() -> Self {
        Self {
            l1_start_epoch: Epoch {
                hash: hash("0x6ffc1bf3754c01f6bb9fe057c1578b87a8571ce2e9be5ca14bace6eccfd336c7"),
                number: 8300214,
                timestamp: 1673550516,
            },
            l2_genesis: BlockInfo {
                hash: hash("0x0f783549ea4313b784eadd9b8e8a69913b368b7366363ea814d7707ac505175f"),
                number: 4061224,
                parent_hash: hash(
                    "0x31267a44f1422f4cab59b076548c075e79bd59e691a23fbce027f572a2a49dc9",
                ),
                timestamp: 1673550516,
            },
            batch_sender: addr("0x7431310e026b69bfc676c0013e12a1a11411eec9"),
            batch_inbox: addr("0xff00000000000000000000000000000000000420"),
            deposit_contract: addr("0x5b47E1A08Ea6d985D6649300584e6722Ec4B1383"),
            max_channels: 100_000_000,
            max_timeout: 100,
            seq_window_size: 120,
            max_seq_drif: 3600,
        }
    }
}

impl Default for SystemAccounts {
    fn default() -> Self {
        Self {
            attributes_depositor: addr("0xdeaddeaddeaddeaddeaddeaddeaddeaddead0001"),
            attributes_predeploy: addr("0x4200000000000000000000000000000000000015"),
            fee_vault: addr("0x4200000000000000000000000000000000000011"),
        }
    }
}

fn addr(s: &str) -> Address {
    Address::from_str(s).unwrap()
}

fn hash(s: &str) -> H256 {
    H256::from_str(s).unwrap()
}
