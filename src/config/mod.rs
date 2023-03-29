use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    process::exit,
    str::FromStr,
};

use ethers_core::types::{Address, H256};
use figment::{
    providers::{Format, Serialized, Toml},
    value::Value,
    Figment,
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
    /// The L2 engine API URL
    pub l2_engine_url: Option<String>,
    /// The base chain config
    pub chain: ChainConfig,
    /// Location of the database folder
    pub data_dir: Option<PathBuf>,
    /// Engine API JWT Secret
    /// This is used to authenticate with the engine API
    pub jwt_secret: Option<String>,
}

pub struct CliConfig {}

impl Config {
    pub fn get_engine_api_url(&self) -> String {
        self.l2_engine_url
            .clone()
            .unwrap_or("http://localhost:8551".to_string())
    }
}

impl Config {
    pub fn new(
        config_path: &PathBuf,
        cli_provider: Serialized<HashMap<&str, Value>>,
        chain: ChainConfig,
    ) -> Self {
        let chain_provider = chain.as_provider();
        let toml_provider = Toml::file(config_path).nested();

        let config_res = Figment::new()
            .merge(chain_provider)
            .merge(toml_provider)
            .merge(cli_provider)
            .extract();

        match config_res {
            Ok(config) => config,
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
}

/// A Chain Configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ChainConfig {
    /// The network name
    pub network: String,
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
    pub max_seq_drift: u64,
    /// Timestamp of the regolith hardfork
    pub regolith_time: u64,
    /// RPC URL of the sequencer
    pub sequencer_rpc: String,
}

fn address_to_str(address: &Address) -> String {
    format!("0x{}", hex::encode(address.as_bytes()))
}

impl ChainConfig {
    pub fn as_provider(&self) -> Serialized<HashMap<&str, Value>> {
        let mut dict = HashMap::new();
        let value = Value::from(self);
        dict.insert("chain", value);
        Serialized::from(dict, "default".to_string())
    }

    fn as_dict(&self) -> BTreeMap<String, Value> {
        let mut dict = BTreeMap::new();
        dict.insert(
            "l1_start_epoch".to_string(),
            Value::from(self.l1_start_epoch),
        );
        dict.insert("l2_genesis".to_string(), Value::from(self.l2_genesis));
        dict.insert(
            "batch_sender".to_string(),
            Value::from(address_to_str(&self.batch_sender)),
        );
        dict.insert(
            "batch_inbox".to_string(),
            Value::from(address_to_str(&self.batch_inbox)),
        );
        dict.insert(
            "deposit_contract".to_string(),
            Value::from(address_to_str(&self.deposit_contract)),
        );
        dict.insert("max_channels".to_string(), Value::from(self.max_channels));
        dict.insert("max_timeout".to_string(), Value::from(self.max_timeout));
        dict.insert(
            "seq_window_size".to_string(),
            Value::from(self.seq_window_size),
        );
        dict.insert("max_seq_drift".to_string(), Value::from(self.max_seq_drift));
        dict.insert("regolith_time".to_string(), Value::from(self.regolith_time));
        dict.insert("network".to_string(), Value::from(self.network.clone()));
        dict.insert("sequencer_rpc".to_string(), Value::from(self.sequencer_rpc.clone()));
        dict
    }
}

impl From<&ChainConfig> for Value {
    fn from(value: &ChainConfig) -> Self {
        Value::from(value.as_dict())
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
    pub fn optimism_goerli() -> Self {
        Self {
            network: "optimism-goerli".to_string(),
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
            seq_window_size: 3600,
            max_seq_drift: 600,
            regolith_time: 1679079600,
            sequencer_rpc: "https://goerli-sequencer.optimism.io".to_string(),
        }
    }
    pub fn base_goerli() -> Self {
        Self {
            network: "base-goerli".to_string(),
            l1_start_epoch: Epoch {
                number: 8410981,
                hash: hash("0x73d89754a1e0387b89520d989d3be9c37c1f32495a88faf1ea05c61121ab0d19"),
                timestamp: 1675193616,
            },
            l2_genesis: BlockInfo {
                hash: hash("0xa3ab140f15ea7f7443a4702da64c10314eb04d488e72974e02e2d728096b4f76"),
                number: 0,
                parent_hash: H256::zero(),
                timestamp: 1675193616,
            },
            batch_sender: addr("0x2d679b567db6187c0c8323fa982cfb88b74dbcc7"),
            batch_inbox: addr("0x8453100000000000000000000000000000000000"),
            deposit_contract: addr("0xe93c8cd0d409341205a592f8c4ac1a5fe5585cfa"),
            max_channels: 100_000_000,
            max_timeout: 100,
            seq_window_size: 3600,
            max_seq_drift: 600,
            regolith_time: u64::MAX,
            sequencer_rpc: "https://goerli.base.org".to_string(),
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
