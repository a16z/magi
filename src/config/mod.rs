use std::{collections::HashMap, path::PathBuf, process::exit, str::FromStr};

use ethers_core::types::{Address, H256};
use figment::{
    providers::{Format, Serialized, Toml},
    value::Value,
    Figment,
};
use serde::Deserialize;

use crate::common::BlockID;

/// Sync Mode Specifies how `magi` should sync the L2 chain
#[derive(Debug, Clone)]
pub enum SyncMode {
    /// Fast sync mode
    Fast,
    /// Challenge sync mode
    ///
    /// This mode will pull **finalized** blocks from the L2 RPC and then derive safe blocks.
    Challenge,
    /// Full sync mode
    Full,
}

/// A system configuration
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// The base chain RPC URL
    pub l1_rpc_url: String,
    /// An External L2 RPC URL that can be used for fast syncing
    pub l2_rpc_url: Option<String>,
    /// The base chain config
    pub chain: ChainConfig,
    /// The maximum number of intermediate pending channels
    pub max_channels: usize,
    /// The max timeout for a channel (as measured by the frame L1 block number)
    pub max_timeout: u64,
    /// Engine API URL
    pub engine_api_url: Option<String>,
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
        let toml_provider = Toml::file(config_path).nested();
        let cli_provider = cli_config.as_provider();
        let config_res = Figment::new()
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
                        println!("\talternatively, you can add the field to your helios.toml file or as an environment variable");
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
        user_dict.insert("max_channels", Value::from(self.max_channels));
        user_dict.insert("max_timeout", Value::from(self.max_timeout));
        if let Some(engine_api_url) = &self.engine_api_url {
            user_dict.insert("engine_api_url", Value::from(engine_api_url.clone()));
        }
        if let Some(jwt_secret) = &self.jwt_secret {
            user_dict.insert("jwt_secret", Value::from(jwt_secret.clone()));
        }

        // TODO: serialize chain config

        Serialized::from(user_dict, "default".to_string())
    }
}

/// A Chain Configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ChainConfig {
    /// The L1 block referenced by the L2 chain
    pub l1_start_epoch: BlockID,
    /// The L2 genesis block
    pub l2_genesis: BlockID,
    /// The batch sender address
    pub batch_sender: Address,
    /// The batch inbox address
    pub batch_inbox: Address,
    /// The deposit contract address
    pub deposit_contract: Address,
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
            l1_start_epoch: BlockID {
                hash: hash("0x6ffc1bf3754c01f6bb9fe057c1578b87a8571ce2e9be5ca14bace6eccfd336c7"),
                parent_hash: hash(
                    "0xf8b68fcad82739208baff929ef51dff682b5960a57cad693babe01b23fd65460",
                ),
                number: 8300214,
            },
            l2_genesis: BlockID {
                hash: hash("0x0f783549ea4313b784eadd9b8e8a69913b368b7366363ea814d7707ac505175f"),
                number: 4061224,
                parent_hash: hash(
                    "0x31267a44f1422f4cab59b076548c075e79bd59e691a23fbce027f572a2a49dc9",
                ),
            },
            batch_sender: addr("0x7431310e026b69bfc676c0013e12a1a11411eec9"),
            batch_inbox: addr("0xff00000000000000000000000000000000000420"),
            deposit_contract: addr("0x5b47E1A08Ea6d985D6649300584e6722Ec4B1383"),
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
