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
    /// Checkpoint sync mode
    Checkpoint,
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
            "checkpoint" => Ok(Self::Checkpoint),
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
    /// The L2 chain RPC URL
    pub l2_rpc_url: String,
    /// The L2 engine API URL
    pub l2_engine_url: String,
    /// The base chain config
    pub chain: ChainConfig,
    /// Engine API JWT Secret
    /// This is used to authenticate with the engine API
    pub jwt_secret: String,
    /// A trusted L2 RPC URL to use for fast/checkpoint syncing
    pub checkpoint_sync_url: Option<String>,
    /// The port of RPC server
    pub rpc_port: u16,
    /// The devnet mode.
    pub devnet: bool,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint_sync_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rpc_port: Option<u16>,
    #[serde(default)]
    pub devnet: bool,
}

/// A Chain Configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    /// The network name
    pub network: String,
    /// The L1 chain id
    pub l1_chain_id: u64,
    /// The L2 chain id
    pub l2_chain_id: u64,
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
    /// Protocol meta configuration
    pub meta: ProtocolMetaConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolMetaConfig {
    pub enable_config_updates: bool,
    pub enable_user_deposited_txs: bool,
}

impl ProtocolMetaConfig {
    pub fn optimism() -> Self {
        Self {
            enable_config_updates: true,
            enable_user_deposited_txs: true,
        }
    }
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
    /// Sequencer's signer for unsafe blocks
    pub unsafe_block_signer: Address,
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
    rpc_port: u16,
}

impl Default for DefaultsProvider {
    fn default() -> Self {
        Self {
            l2_rpc_url: "http://127.0.0.1:8545".to_string(),
            l2_engine_url: "http://127.0.0.1:8551".to_string(),
            rpc_port: 9545,
        }
    }
}

impl ChainConfig {
    /// Read and parse a chain config object from a JSON file path
    pub fn from_json(path: &str) -> Self {
        let file = std::fs::File::open(path).unwrap();
        let external: ExternalChainConfig = serde_json::from_reader(file).unwrap();
        external.into()
    }

    pub fn optimism() -> Self {
        Self {
            network: "optimism".to_string(),
            l1_chain_id: 1,
            l2_chain_id: 10,
            l1_start_epoch: Epoch {
                hash: hash("0x438335a20d98863a4c0c97999eb2481921ccd28553eac6f913af7c12aec04108"),
                number: 17422590,
                timestamp: 1686068903,
            },
            l2_genesis: BlockInfo {
                hash: hash("0xdbf6a80fef073de06add9b0d14026d6e5a86c85f6d102c36d3d8e9cf89c2afd3"),
                number: 105235063,
                parent_hash: hash(
                    "0x21a168dfa5e727926063a28ba16fd5ee84c814e847c81a699c7a0ea551e4ca50",
                ),
                timestamp: 1686068903,
            },
            system_config: SystemConfig {
                batch_sender: addr("0x6887246668a3b87f54deb3b94ba47a6f63f32985"),
                gas_limit: U256::from(30_000_000),
                l1_fee_overhead: U256::from(188),
                l1_fee_scalar: U256::from(684000),
                unsafe_block_signer: addr("0xAAAA45d9549EDA09E70937013520214382Ffc4A2"),
            },
            batch_inbox: addr("0xff00000000000000000000000000000000000010"),
            deposit_contract: addr("0xbEb5Fc579115071764c7423A4f12eDde41f106Ed"),
            system_config_contract: addr("0x229047fed2591dbec1eF1118d64F7aF3dB9EB290"),
            l2_to_l1_message_passer: addr("0x4200000000000000000000000000000000000016"),
            max_channel_size: 100_000_000,
            channel_timeout: 300,
            seq_window_size: 3600,
            max_seq_drift: 600,
            blocktime: 2,
            regolith_time: 0,
            meta: ProtocolMetaConfig::optimism(),
        }
    }

    pub fn optimism_goerli() -> Self {
        Self {
            network: "optimism-goerli".to_string(),
            l1_chain_id: 5,
            l2_chain_id: 420,
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
                unsafe_block_signer: addr("0x715b7219D986641DF9eFd9C7Ef01218D528e19ec"),
            },
            system_config_contract: addr("0xAe851f927Ee40dE99aaBb7461C00f9622ab91d60"),
            batch_inbox: addr("0xff00000000000000000000000000000000000420"),
            deposit_contract: addr("0x5b47E1A08Ea6d985D6649300584e6722Ec4B1383"),
            l2_to_l1_message_passer: addr("0xEF2ec5A5465f075E010BE70966a8667c94BCe15a"),
            max_channel_size: 100_000_000,
            channel_timeout: 300,
            seq_window_size: 3600,
            max_seq_drift: 600,
            regolith_time: 1679079600,
            blocktime: 2,
            meta: ProtocolMetaConfig::optimism(),
        }
    }
    pub fn optimism_sepolia() -> Self {
        Self {
            network: "optimism-sepolia".to_string(),
            l1_chain_id: 11155111,
            l2_chain_id: 11155420,
            l1_start_epoch: Epoch {
                hash: hash("0x48f520cf4ddaf34c8336e6e490632ea3cf1e5e93b0b2bc6e917557e31845371b"),
                number: 4071408,
                timestamp: 1691802540,
            },
            l2_genesis: BlockInfo {
                hash: hash("0x102de6ffb001480cc9b8b548fd05c34cd4f46ae4aa91759393db90ea0409887d"),
                number: 0,
                parent_hash: hash(
                    "0x0000000000000000000000000000000000000000000000000000000000000000",
                ),
                timestamp: 1691802540,
            },
            system_config: SystemConfig {
                batch_sender: addr("0x8F23BB38F531600e5d8FDDaAEC41F13FaB46E98c"),
                gas_limit: U256::from(30_000_000),
                l1_fee_overhead: U256::from(188),
                l1_fee_scalar: U256::from(684000),
                unsafe_block_signer: addr("0x0000000000000000000000000000000000000000"),
            },
            system_config_contract: addr("0x034edd2a225f7f429a63e0f1d2084b9e0a93b538"),
            batch_inbox: addr("0xff00000000000000000000000000000011155420"),
            deposit_contract: addr("0x16fc5058f25648194471939df75cf27a2fdc48bc"),
            l2_to_l1_message_passer: addr("0x4200000000000000000000000000000000000016"),
            max_channel_size: 100_000_000,
            channel_timeout: 300,
            seq_window_size: 3600,
            max_seq_drift: 600,
            regolith_time: 0,
            blocktime: 2,
            meta: ProtocolMetaConfig::optimism(),
        }
    }

    pub fn base() -> Self {
        Self {
            network: "base".to_string(),
            l1_chain_id: 1,
            l2_chain_id: 8453,
            l1_start_epoch: Epoch {
                number: 17481768,
                hash: hash("0x5c13d307623a926cd31415036c8b7fa14572f9dac64528e857a470511fc30771"),
                timestamp: 1686789347,
            },
            l2_genesis: BlockInfo {
                hash: hash("0xf712aa9241cc24369b143cf6dce85f0902a9731e70d66818a3a5845b296c73dd"),
                number: 0,
                parent_hash: H256::zero(),
                timestamp: 1686789347,
            },
            system_config: SystemConfig {
                batch_sender: addr("0x5050f69a9786f081509234f1a7f4684b5e5b76c9"),
                gas_limit: U256::from(30000000),
                l1_fee_overhead: U256::from(188),
                l1_fee_scalar: U256::from(684000),
                unsafe_block_signer: addr("0xAf6E19BE0F9cE7f8afd49a1824851023A8249e8a"),
            },
            batch_inbox: addr("0xff00000000000000000000000000000000008453"),
            deposit_contract: addr("0x49048044d57e1c92a77f79988d21fa8faf74e97e"),
            system_config_contract: addr("0x73a79fab69143498ed3712e519a88a918e1f4072"),
            l2_to_l1_message_passer: addr("0x4200000000000000000000000000000000000016"),
            max_channel_size: 100_000_000,
            channel_timeout: 300,
            seq_window_size: 3600,
            max_seq_drift: 600,
            blocktime: 2,
            regolith_time: 0,
            meta: ProtocolMetaConfig::optimism(),
        }
    }

    pub fn base_goerli() -> Self {
        Self {
            network: "base-goerli".to_string(),
            l1_chain_id: 5,
            l2_chain_id: 84531,
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
                unsafe_block_signer: addr("0x32a4e99A72c11E9DD3dC159909a2D7BD86C1Bc51"),
            },
            system_config_contract: addr("0xb15eea247ece011c68a614e4a77ad648ff495bc1"),
            batch_inbox: addr("0x8453100000000000000000000000000000000000"),
            deposit_contract: addr("0xe93c8cd0d409341205a592f8c4ac1a5fe5585cfa"),
            l2_to_l1_message_passer: addr("0x4200000000000000000000000000000000000016"),
            max_channel_size: 100_000_000,
            channel_timeout: 100,
            seq_window_size: 3600,
            max_seq_drift: 600,
            regolith_time: 1683219600,
            blocktime: 2,
            meta: ProtocolMetaConfig::optimism(),
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

/// External chain config
///
/// This is used to parse external chain configs from JSON.
/// This interface corresponds to the default output of the `op-node`
/// genesis devnet setup command `--outfile.rollup` flag.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExternalChainConfig {
    genesis: ExternalGenesisInfo,
    block_time: u64,
    max_sequencer_drift: u64,
    seq_window_size: u64,
    channel_timeout: u64,
    l1_chain_id: u64,
    l2_chain_id: u64,
    regolith_time: u64,
    batch_inbox_address: Address,
    deposit_contract_address: Address,
    l1_system_config_address: Address,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExternalGenesisInfo {
    l1: ChainGenesisInfo,
    l2: ChainGenesisInfo,
    l2_time: u64,
    system_config: SystemConfigInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SystemConfigInfo {
    #[serde(rename = "batcherAddr")]
    batcher_addr: Address,
    overhead: H256,
    scalar: H256,
    #[serde(rename = "gasLimit")]
    gas_limit: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChainGenesisInfo {
    hash: H256,
    number: u64,
}

impl From<ExternalChainConfig> for ChainConfig {
    fn from(external: ExternalChainConfig) -> Self {
        Self {
            network: "external".to_string(),
            l1_chain_id: external.l1_chain_id,
            l2_chain_id: external.l2_chain_id,
            l1_start_epoch: Epoch {
                hash: external.genesis.l1.hash,
                number: external.genesis.l1.number,
                timestamp: 0,
            },
            l2_genesis: BlockInfo {
                hash: external.genesis.l2.hash,
                number: external.genesis.l2.number,
                parent_hash: H256::zero(),
                timestamp: external.genesis.l2_time,
            },
            system_config: SystemConfig {
                batch_sender: external.genesis.system_config.batcher_addr,
                gas_limit: U256::from(external.genesis.system_config.gas_limit),
                l1_fee_overhead: external.genesis.system_config.overhead.0.into(),
                l1_fee_scalar: external.genesis.system_config.scalar.0.into(),
                unsafe_block_signer: Address::zero(),
            },
            batch_inbox: external.batch_inbox_address,
            deposit_contract: external.deposit_contract_address,
            system_config_contract: external.l1_system_config_address,
            max_channel_size: 100_000_000,
            channel_timeout: external.channel_timeout,
            seq_window_size: external.seq_window_size,
            max_seq_drift: external.max_sequencer_drift,
            regolith_time: external.regolith_time,
            blocktime: external.block_time,
            l2_to_l1_message_passer: Address::zero(),
            meta: ProtocolMetaConfig::optimism(),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_read_external_chain_from_json() {
        let devnet_json = r#"
        {
            "genesis": {
              "l1": {
                "hash": "0xdb52a58e7341447d1a9525d248ea07dbca7dfa0e105721dee1aa5a86163c088d",
                "number": 0
              },
              "l2": {
                "hash": "0xf85bca315a08237644b06a8350cda3bc0de1593745a91be93daeadb28fb3a32e",
                "number": 0
              },
              "l2_time": 1685710775,
              "system_config": {
                "batcherAddr": "0x3c44cdddb6a900fa2b585dd299e03d12fa4293bc",
                "overhead": "0x0000000000000000000000000000000000000000000000000000000000000834",
                "scalar": "0x00000000000000000000000000000000000000000000000000000000000f4240",
                "gasLimit": 30000000
              }
            },
            "block_time": 2,
            "max_sequencer_drift": 300,
            "seq_window_size": 200,
            "channel_timeout": 120,
            "l1_chain_id": 900,
            "l2_chain_id": 901,
            "regolith_time": 0,
            "batch_inbox_address": "0xff00000000000000000000000000000000000000",
            "deposit_contract_address": "0x6900000000000000000000000000000000000001",
            "l1_system_config_address": "0x6900000000000000000000000000000000000009"
          }          
        "#;

        let external: ExternalChainConfig = serde_json::from_str(devnet_json).unwrap();
        let chain: ChainConfig = external.into();

        assert_eq!(chain.network, "external");
        assert_eq!(chain.l1_chain_id, 900);
        assert_eq!(chain.l2_chain_id, 901);
        assert_eq!(chain.l1_start_epoch.number, 0);
        assert_eq!(
            chain.l1_start_epoch.hash,
            hash("0xdb52a58e7341447d1a9525d248ea07dbca7dfa0e105721dee1aa5a86163c088d")
        );
        assert_eq!(chain.l2_genesis.number, 0);
        assert_eq!(
            chain.l2_genesis.hash,
            hash("0xf85bca315a08237644b06a8350cda3bc0de1593745a91be93daeadb28fb3a32e")
        );
        assert_eq!(chain.system_config.gas_limit, U256::from(30_000_000));
        assert_eq!(chain.system_config.l1_fee_overhead, U256::from(2100));
        assert_eq!(chain.system_config.l1_fee_scalar, U256::from(1_000_000));
        assert_eq!(
            chain.system_config.batch_sender,
            addr("0x3c44cdddb6a900fa2b585dd299e03d12fa4293bc")
        );
        assert_eq!(
            chain.batch_inbox,
            addr("0xff00000000000000000000000000000000000000")
        );
        assert_eq!(
            chain.deposit_contract,
            addr("0x6900000000000000000000000000000000000001")
        );
        assert_eq!(
            chain.system_config_contract,
            addr("0x6900000000000000000000000000000000000009")
        );
        assert_eq!(chain.max_channel_size, 100_000_000);
        assert_eq!(chain.channel_timeout, 120);
        assert_eq!(chain.seq_window_size, 200);
        assert_eq!(chain.max_seq_drift, 300);
        assert_eq!(chain.regolith_time, 0);
        assert_eq!(chain.blocktime, 2);
    }
}
