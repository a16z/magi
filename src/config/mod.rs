use std::{iter, path::PathBuf, process::exit, str::FromStr};

use ethers::types::{Address, H256, U256};
use figment::{
    providers::{Format, Serialized, Toml},
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
    pub l2_rpc_url: String,
    /// The L2 engine API URL
    pub l2_engine_url: String,
    /// The base chain config
    pub chain: ChainConfig,
    /// Engine API JWT Secret
    /// This is used to authenticate with the engine API
    pub jwt_secret: String,
}

impl Config {
    pub fn new(config_path: &PathBuf, cli_config: CliConfig, chain: ChainConfig) -> Self {
        let defaults_provider = Serialized::defaults(DefaultsProvider::default());
        let chain_provider: Serialized<ChainProvider> = chain.into();
        let toml_provider = Toml::file(config_path).nested();
        let cli_provider = Serialized::defaults(cli_config);

        let config_res = Figment::new()
            .merge(defaults_provider)
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

/// Chain config items derived from the CLI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub l1_rpc_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub l2_rpc_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub l2_engine_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwt_secret: Option<String>,
}

/// A Chain Configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    /// The network name
    pub network: String,
    /// The L1 block referenced by the L2 chain
    pub l1_start_epoch: Epoch,
    /// The L2 genesis block info
    pub l2_genesis: BlockInfo,
    /// The initial system config value
    pub system_config: SystemConfig,
    /// The batch inbox address
    pub batch_inbox: Address,
    /// The deposit contract address
    pub deposit_contract: Address,
    /// The L1 system config contract
    pub system_config_contract: Address,
    /// The L2 output oracle contract on L1
    pub l2_output_oracle: Address,
    /// The maximum byte size of all pending channels
    pub max_channel_size: u64,
    /// The max timeout for a channel (as measured by the frame L1 block number)
    pub channel_timeout: u64,
    /// Number of L1 blocks in a sequence window
    pub seq_window_size: u64,
    /// Maximum timestamp drift
    pub max_seq_drift: u64,
    /// Timestamp of the regolith hardfork
    pub regolith_time: u64,
    /// Network blocktime
    #[serde(default = "default_blocktime")]
    pub blocktime: u64,
    /// L2 To L1 Message passer address
    pub l2_to_l1_message_passer: Address,
}

/// Optimism system config contract values
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SystemConfig {
    /// Batch sender address
    pub batch_sender: Address,
    /// L2 gas limit
    pub gas_limit: U256,
    /// Fee overhead
    pub l1_fee_overhead: U256,
    /// Fee scalar
    pub l1_fee_scalar: U256,
}

impl SystemConfig {
    /// Encoded batch sender as a H256
    pub fn batcher_hash(&self) -> H256 {
        let mut batch_sender_bytes = self.batch_sender.as_bytes().to_vec();
        let mut batcher_hash = iter::repeat(0).take(12).collect::<Vec<_>>();
        batcher_hash.append(&mut batch_sender_bytes);
        H256::from_slice(&batcher_hash)
    }
}

/// System accounts
#[derive(Debug, Clone)]
pub struct SystemAccounts {
    pub attributes_depositor: Address,
    pub attributes_predeploy: Address,
    pub fee_vault: Address,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChainProvider {
    chain: ChainConfig,
}

impl From<ChainConfig> for Serialized<ChainProvider> {
    fn from(value: ChainConfig) -> Self {
        Serialized::defaults(ChainProvider { chain: value })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DefaultsProvider {
    l2_rpc_url: String,
    l2_engine_url: String,
}

impl Default for DefaultsProvider {
    fn default() -> Self {
        Self {
            l2_rpc_url: "http://127.0.0.1:8545".to_string(),
            l2_engine_url: "http://127.0.0.1:8551".to_string(),
        }
    }
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
            system_config: SystemConfig {
                batch_sender: addr("0x7431310e026b69bfc676c0013e12a1a11411eec9"),
                gas_limit: U256::from(25_000_000),
                l1_fee_overhead: U256::from(2100),
                l1_fee_scalar: U256::from(1000000),
            },
            system_config_contract: addr("0xAe851f927Ee40dE99aaBb7461C00f9622ab91d60"),
            batch_inbox: addr("0xff00000000000000000000000000000000000420"),
            deposit_contract: addr("0x5b47E1A08Ea6d985D6649300584e6722Ec4B1383"),
            l2_output_oracle: addr("0xE6Dfba0953616Bacab0c9A8ecb3a9BBa77FC15c0"),
            l2_to_l1_message_passer: addr("0xEF2ec5A5465f075E010BE70966a8667c94BCe15a"),
            max_channel_size: 100_000_000,
            channel_timeout: 300,
            seq_window_size: 3600,
            max_seq_drift: 600,
            regolith_time: 1679079600,
            blocktime: 2,
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
            system_config: SystemConfig {
                batch_sender: addr("0x2d679b567db6187c0c8323fa982cfb88b74dbcc7"),
                gas_limit: U256::from(25_000_000),
                l1_fee_overhead: U256::from(2100),
                l1_fee_scalar: U256::from(1000000),
            },
            system_config_contract: addr("0xb15eea247ece011c68a614e4a77ad648ff495bc1"),
            batch_inbox: addr("0x8453100000000000000000000000000000000000"),
            deposit_contract: addr("0xe93c8cd0d409341205a592f8c4ac1a5fe5585cfa"),
            l2_output_oracle: addr("0x2A35891ff30313CcFa6CE88dcf3858bb075A2298"),
            l2_to_l1_message_passer: addr("0x4200000000000000000000000000000000000016"),
            max_channel_size: 100_000_000,
            channel_timeout: 100,
            seq_window_size: 3600,
            max_seq_drift: 600,
            regolith_time: u64::MAX,
            blocktime: 2,
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

fn default_blocktime() -> u64 {
    2
}
