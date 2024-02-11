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

/// The global `Magi` configuration.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    /// The L1 chain RPC URL
    pub l1_rpc_url: String,
    /// The base chain beacon client RPC URL
    pub l1_beacon_url: String,
    /// The L2 chain RPC URL
    pub l2_rpc_url: String,
    /// The L2 engine API URL
    pub l2_engine_url: String,
    /// The L2 chain config
    pub chain: ChainConfig,
    /// Engine API JWT Secret.
    /// This is used to authenticate with the engine API
    pub jwt_secret: String,
    /// A trusted L2 RPC URL to use for fast/checkpoint syncing
    pub checkpoint_sync_url: Option<String>,
    /// The port of the `Magi` RPC server
    pub rpc_port: u16,
    /// The socket address of RPC server
    pub rpc_addr: String,
    /// The devnet mode.
    /// If devnet is enabled.
    pub devnet: bool,
}

impl Config {
    /// Creates a new [Config], based on a config TOML and/or CLI flags.
    ///
    /// If a setting exists in the TOML and is also passed via CLI, the CLI will take priority.
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
                        println!("\n\ttry supplying the proper command line argument: --{field}");
                        println!("\talternatively, you can add the field to your magi.toml file");
                        println!("\nfor more information, check the github README");
                    }
                    _ => println!("cannot parse configuration: {err}"),
                }
                exit(1);
            }
        }
    }
}

/// Magi config items derived from the CLI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliConfig {
    /// The L1 RPC
    #[serde(skip_serializing_if = "Option::is_none")]
    pub l1_rpc_url: Option<String>,
    /// The L1 beacon client RPC
    #[serde(skip_serializing_if = "Option::is_none")]
    pub l1_beacon_url: Option<String>,
    /// The L2 execution client RPC
    #[serde(skip_serializing_if = "Option::is_none")]
    pub l2_rpc_url: Option<String>,
    /// The L2 engine RPC
    #[serde(skip_serializing_if = "Option::is_none")]
    pub l2_engine_url: Option<String>,
    /// The JWT secret used to authenticate with the engine
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwt_secret: Option<String>,
    /// A trusted L2 RPC used to obtain data from when using checkpoint sync mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint_sync_url: Option<String>,
    /// The port to serve the Magi RPC on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rpc_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rpc_addr: Option<String>,
    /// If Magi is running in devnet mode.
    #[serde(default)]
    pub devnet: bool,
}

/// Configurations for a blockchain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    /// The network name
    pub network: String,
    /// The L1 chain id
    pub l1_chain_id: u64,
    /// The L2 chain id
    pub l2_chain_id: u64,
    /// The L1 genesis block referenced by the L2 chain
    pub l1_start_epoch: Epoch,
    /// The L2 genesis block info
    pub l2_genesis: BlockInfo,
    /// The initial system config value
    pub system_config: SystemConfig,
    /// The batch inbox address
    pub batch_inbox: Address,
    /// The deposit contract address
    pub deposit_contract: Address,
    /// The L1 system config contract address
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
    /// Timestamp of the canyon hardfork
    pub canyon_time: u64,
    /// Timestamp of the delta hardfork
    pub delta_time: u64,
    /// Timestamp of the ecotone hardfork
    pub ecotone_time: u64,
    /// Network blocktime
    #[serde(default = "default_blocktime")]
    pub blocktime: u64,
    /// L2 To L1 Message passer address
    pub l2_to_l1_message_passer: Address,
}

impl Default for ChainConfig {
    /// Defaults to the Optimism [ChainConfig]
    fn default() -> Self {
        ChainConfig::optimism()
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
    /// Encodes batch sender as a H256
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
    /// The address that submits attributes deposited transactions in every L2 block
    pub attributes_depositor: Address,
    /// The contract address that attributes deposited transactions are submitted to
    pub attributes_predeploy: Address,
    /// The contract address that holds fees paid to the sequencer during transaction execution & block production
    pub fee_vault: Address,
}

/// Wrapper around a [ChainConfig]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChainProvider {
    /// The [ChainConfig] which is unique for each blockchain
    chain: ChainConfig,
}

impl From<ChainConfig> for Serialized<ChainProvider> {
    fn from(value: ChainConfig) -> Self {
        Serialized::defaults(ChainProvider { chain: value })
    }
}

/// Provides default values for the L2 RPC & engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DefaultsProvider {
    /// The L2 execution node RPC
    l2_rpc_url: String,
    /// The L2 engine RPC
    l2_engine_url: String,
    /// The port to serve the Magi RPC server on
    rpc_port: u16,
    rpc_addr: String,
}

impl Default for DefaultsProvider {
    /// Provides default values for the L2 RPC & engine.
    fn default() -> Self {
        Self {
            l2_rpc_url: "http://127.0.0.1:8545".to_string(),
            l2_engine_url: "http://127.0.0.1:8551".to_string(),
            rpc_port: 9545,
            rpc_addr: "127.0.0.1".to_string(),
        }
    }
}

impl ChainConfig {
    /// Read and parse the [ChainConfig] from a JSON file path
    pub fn from_json(path: &str) -> Self {
        let file = std::fs::File::open(path).unwrap();
        let external: ExternalChainConfig = serde_json::from_reader(file).unwrap();
        external.into()
    }

    /// Generates a [ChainConfig] instance from a given network name.
    pub fn from_network_name(network: &str) -> Self {
        match network.to_lowercase().as_str() {
            "optimism" => Self::optimism(),
            "optimism-goerli" => Self::optimism_goerli(),
            "optimism-sepolia" => Self::optimism_sepolia(),
            "base" => Self::base(),
            "base-goerli" => Self::base_goerli(),
            "base-sepolia" => Self::base_sepolia(),
            file if file.ends_with(".json") => Self::from_json(file),
            _ => panic!(
                "Invalid network name. \\
            Please use one of the following: 'optimism', 'optimism-goerli', 'optimism-sepolia', 'base-goerli', 'base-sepolia', 'base'. \\
            You can also use a JSON file path for custom configuration."
            ),
        }
    }

    /// Returns true if the block is the first block subject to the Ecotone hardfork
    pub fn is_ecotone_activation_block(&self, block_time: u64) -> bool {
        if block_time < self.blocktime {
            return false;
        }

        block_time - self.blocktime < self.ecotone_time
    }

    /// Returns true if Ecotone hardfork is active but the block is not the
    /// first block subject to the hardfork. Ecotone activation at genesis does not count.
    pub fn is_ecotone_but_not_first_block(&self, block_time: u64) -> bool {
        let is_ecotone = block_time >= self.ecotone_time;

        is_ecotone && !self.is_ecotone_activation_block(block_time)
    }

    /// [ChainConfig] for Optimism
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
            canyon_time: 170499240,
            delta_time: 1708560000,
            ecotone_time: 1710781201,
        }
    }

    /// [ChainConfig] for Optimism Goerli
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
            canyon_time: 1699981200,
            delta_time: 1703116800,
            ecotone_time: 1707238800,
            blocktime: 2,
        }
    }

    /// [ChainConfig] for Optimism Sepolia
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
                unsafe_block_signer: addr("0x57CACBB0d30b01eb2462e5dC940c161aff3230D3"),
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
            canyon_time: 1699981200,
            delta_time: 1703203200,
            ecotone_time: 1708534800,
            blocktime: 2,
        }
    }

    /// [ChainConfig] for Base
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
            canyon_time: 1704992401,
            delta_time: 1708560000,
            ecotone_time: 1710781201,
        }
    }

    /// [ChainConfig] for Base Goerli
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
            channel_timeout: 300,
            seq_window_size: 3600,
            max_seq_drift: 600,
            regolith_time: 1683219600,
            canyon_time: 1699981200,
            delta_time: 1703116800,
            ecotone_time: 1707238800,
            blocktime: 2,
        }
    }

    /// [ChainConfig] for Base Sepolia
    pub fn base_sepolia() -> Self {
        Self {
            network: "base-sepolia".to_string(),
            l1_chain_id: 11155111,
            l2_chain_id: 84532,
            l1_start_epoch: Epoch {
                number: 4370868,
                hash: hash("0xcac9a83291d4dec146d6f7f69ab2304f23f5be87b1789119a0c5b1e4482444ed"),
                timestamp: 1695768288,
            },
            l2_genesis: BlockInfo {
                hash: hash("0x0dcc9e089e30b90ddfc55be9a37dd15bc551aeee999d2e2b51414c54eaf934e4"),
                number: 0,
                parent_hash: H256::zero(),
                timestamp: 1695768288,
            },
            system_config: SystemConfig {
                batch_sender: addr("0x6cdebe940bc0f26850285caca097c11c33103e47"),
                gas_limit: U256::from(25_000_000),
                l1_fee_overhead: U256::from(2100),
                l1_fee_scalar: U256::from(1000000),
                unsafe_block_signer: addr("0xb830b99c95Ea32300039624Cb567d324D4b1D83C"),
            },
            system_config_contract: addr("0xf272670eb55e895584501d564AfEB048bEd26194"),
            batch_inbox: addr("0xff00000000000000000000000000000000084532"),
            deposit_contract: addr("0x49f53e41452C74589E85cA1677426Ba426459e85"),
            l2_to_l1_message_passer: addr("0x4200000000000000000000000000000000000016"),
            max_channel_size: 100_000_000,
            channel_timeout: 300,
            seq_window_size: 3600,
            max_seq_drift: 600,
            regolith_time: 0,
            canyon_time: 1699981200,
            delta_time: 1703203200,
            ecotone_time: 1708534800,
            blocktime: 2,
        }
    }
}

impl Default for SystemAccounts {
    /// The default system addresses
    fn default() -> Self {
        Self {
            attributes_depositor: addr("0xdeaddeaddeaddeaddeaddeaddeaddeaddead0001"),
            attributes_predeploy: addr("0x4200000000000000000000000000000000000015"),
            fee_vault: addr("0x4200000000000000000000000000000000000011"),
        }
    }
}

/// Converts a [str] to an [Address]
fn addr(s: &str) -> Address {
    Address::from_str(s).unwrap()
}

/// Converts a [str] to a [H256].
fn hash(s: &str) -> H256 {
    H256::from_str(s).unwrap()
}

/// Returns default blocktime of 2 (seconds).
fn default_blocktime() -> u64 {
    2
}

/// External chain config
///
/// This is used to parse external chain configs from JSON.
/// This interface corresponds to the default output of the `op-node`
/// genesis devnet setup command `--outfile.rollup` flag.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalChainConfig {
    /// Genesis settings
    genesis: ExternalGenesisInfo,
    /// Block time of the chain
    block_time: u64,
    /// Maximum timestamp drift
    max_sequencer_drift: u64,
    /// Number of L1 blocks in a sequence window
    seq_window_size: u64,
    /// The max timeout for a channel (as measured by the frame L1 block number)
    channel_timeout: u64,
    /// The L1 chain id
    l1_chain_id: u64,
    /// The L2 chain id
    l2_chain_id: u64,
    /// Timestamp of the regolith hardfork
    regolith_time: u64,
    /// Timestamp of the canyon hardfork
    canyon_time: u64,
    /// Timestamp of the delta hardfork
    delta_time: u64,
    /// Timestamp of the ecotone hardfork
    ecotone_time: u64,
    /// The batch inbox address
    batch_inbox_address: Address,
    /// The deposit contract address
    deposit_contract_address: Address,
    /// The L1 system config contract address
    l1_system_config_address: Address,
}

/// The Genesis property of the `rollup.json` file used in `op-node`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExternalGenesisInfo {
    /// L1 genesis block hash & number
    l1: ChainGenesisInfo,
    /// L2 genesis block hash & number
    l2: ChainGenesisInfo,
    /// L2 genesis block timestamp
    l2_time: u64,
    /// Genesis [SystemConfigInfo] settings
    system_config: SystemConfigInfo,
}

/// System config settings
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SystemConfigInfo {
    /// The authorized batch sender that sends batcher transactions to the batch inbox on L1
    #[serde(rename = "batcherAddr")]
    batcher_addr: Address,
    /// The current L1 fee overhead to apply to L2 transactions cost computation. Unused after Ecotone hard fork.
    overhead: H256,
    /// The current L1 fee scalar to apply to L2 transactions cost computation. Unused after Ecotone hard fork.
    scalar: H256,
    /// The gas limit for L2 blocks
    #[serde(rename = "gasLimit")]
    gas_limit: u64,
}

/// Genesis block hash & number
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChainGenesisInfo {
    /// Genesis block number
    hash: H256,
    /// Genesis block hash
    number: u64,
}

impl From<ExternalChainConfig> for ChainConfig {
    /// Converts an [ExternalChainConfig] to [ChainConfig].
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
            canyon_time: external.canyon_time,
            delta_time: external.delta_time,
            ecotone_time: external.ecotone_time,
            blocktime: external.block_time,
            l2_to_l1_message_passer: addr("0x4200000000000000000000000000000000000016"),
        }
    }
}

impl From<ChainConfig> for ExternalChainConfig {
    /// Converts [ChainConfig] into [ExternalChainConfig]
    /// which is the format used in ``rollup.json`` by `op-node`
    fn from(chain_config: ChainConfig) -> Self {
        let mut overhead = [0; 32];
        let mut scalar = [0; 32];

        chain_config
            .system_config
            .l1_fee_overhead
            .to_big_endian(&mut overhead);
        chain_config
            .system_config
            .l1_fee_scalar
            .to_big_endian(&mut scalar);

        Self {
            genesis: ExternalGenesisInfo {
                l1: ChainGenesisInfo {
                    hash: chain_config.l1_start_epoch.hash,
                    number: chain_config.l1_start_epoch.number,
                },
                l2: ChainGenesisInfo {
                    hash: chain_config.l2_genesis.hash,
                    number: chain_config.l2_genesis.number,
                },
                l2_time: chain_config.l2_genesis.timestamp,
                system_config: SystemConfigInfo {
                    batcher_addr: chain_config.system_config.batch_sender,
                    overhead: H256::from_slice(&overhead),
                    scalar: H256::from_slice(&scalar),
                    gas_limit: chain_config.system_config.gas_limit.as_u64(),
                },
            },
            block_time: chain_config.blocktime,
            max_sequencer_drift: chain_config.max_seq_drift,
            seq_window_size: chain_config.seq_window_size,
            channel_timeout: chain_config.channel_timeout,
            l1_chain_id: chain_config.l1_chain_id,
            l2_chain_id: chain_config.l2_chain_id,
            regolith_time: chain_config.regolith_time,
            canyon_time: chain_config.canyon_time,
            delta_time: chain_config.delta_time,
            ecotone_time: chain_config.ecotone_time,
            batch_inbox_address: chain_config.batch_inbox,
            deposit_contract_address: chain_config.deposit_contract,
            l1_system_config_address: chain_config.system_config_contract,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_chain_config_to_external_chain_config() {
        let chain_config = ChainConfig::optimism();
        let external_config: ExternalChainConfig = chain_config.clone().into();

        assert_eq!(
            external_config.max_sequencer_drift,
            chain_config.max_seq_drift
        );
        assert_eq!(
            external_config.seq_window_size,
            chain_config.seq_window_size
        );
        assert_eq!(
            external_config.channel_timeout,
            chain_config.channel_timeout
        );
        assert_eq!(external_config.l1_chain_id, chain_config.l1_chain_id);
        assert_eq!(external_config.l2_chain_id, chain_config.l2_chain_id);
        assert_eq!(external_config.block_time, chain_config.blocktime);
        assert_eq!(external_config.regolith_time, chain_config.regolith_time);
        assert_eq!(
            external_config.batch_inbox_address,
            chain_config.batch_inbox
        );
        assert_eq!(
            external_config.deposit_contract_address,
            chain_config.deposit_contract
        );
        assert_eq!(
            external_config.l1_system_config_address,
            chain_config.system_config_contract
        );

        assert_eq!(
            external_config.genesis.l1.hash,
            chain_config.l1_start_epoch.hash
        );
        assert_eq!(
            external_config.genesis.l1.number,
            chain_config.l1_start_epoch.number
        );
        assert_eq!(
            external_config.genesis.l2.hash,
            chain_config.l2_genesis.hash
        );
        assert_eq!(
            external_config.genesis.l2.number,
            chain_config.l2_genesis.number
        );
        assert_eq!(
            external_config.genesis.l2_time,
            chain_config.l2_genesis.timestamp
        );

        assert_eq!(
            external_config.genesis.system_config.batcher_addr,
            chain_config.system_config.batch_sender
        );

        let mut overhead = [0; 32];
        let mut scalar = [0; 32];

        chain_config
            .system_config
            .l1_fee_overhead
            .to_big_endian(&mut overhead);
        chain_config
            .system_config
            .l1_fee_scalar
            .to_big_endian(&mut scalar);

        assert_eq!(
            external_config.genesis.system_config.overhead,
            H256::from_slice(&overhead),
        );
        assert_eq!(
            external_config.genesis.system_config.scalar,
            H256::from_slice(&scalar),
        );

        assert_eq!(
            external_config.genesis.system_config.gas_limit,
            chain_config.system_config.gas_limit.as_u64()
        );
    }

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
            "regolith_time": 1,
            "canyon_time": 2,
            "delta_time": 3,
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
        assert_eq!(chain.regolith_time, 1);
        assert_eq!(chain.canyon_time, 2);
        assert_eq!(chain.delta_time, 3);
        assert_eq!(chain.blocktime, 2);
        assert_eq!(
            chain.l2_to_l1_message_passer,
            addr("0x4200000000000000000000000000000000000016")
        );
    }

    #[test]
    fn test_chain_config_from_name() {
        let optimism_config = ChainConfig::optimism();
        let desired_config = ChainConfig::from_network_name("opTimIsm");

        assert_eq!(optimism_config.max_seq_drift, desired_config.max_seq_drift);

        assert_eq!(
            optimism_config.seq_window_size,
            desired_config.seq_window_size
        );
        assert_eq!(
            optimism_config.channel_timeout,
            desired_config.channel_timeout
        );
        assert_eq!(optimism_config.l1_chain_id, desired_config.l1_chain_id);
        assert_eq!(optimism_config.l2_chain_id, desired_config.l2_chain_id);
        assert_eq!(optimism_config.blocktime, desired_config.blocktime);
        assert_eq!(optimism_config.regolith_time, desired_config.regolith_time);
        assert_eq!(optimism_config.batch_inbox, desired_config.batch_inbox);
        assert_eq!(
            optimism_config.deposit_contract,
            desired_config.deposit_contract
        );
        assert_eq!(
            optimism_config.system_config_contract,
            desired_config.system_config_contract
        );

        assert_eq!(
            optimism_config.l1_start_epoch.hash,
            desired_config.l1_start_epoch.hash
        );
        assert_eq!(
            optimism_config.l1_start_epoch.number,
            desired_config.l1_start_epoch.number
        );
        assert_eq!(
            optimism_config.l2_genesis.hash,
            desired_config.l2_genesis.hash
        );
        assert_eq!(
            optimism_config.l2_genesis.number,
            desired_config.l2_genesis.number
        );
        assert_eq!(
            optimism_config.l2_genesis.timestamp,
            desired_config.l2_genesis.timestamp
        );

        assert_eq!(
            optimism_config.system_config.batch_sender,
            desired_config.system_config.batch_sender
        );

        assert_eq!(
            optimism_config.system_config.l1_fee_overhead,
            desired_config.system_config.l1_fee_overhead
        );
        assert_eq!(
            optimism_config.system_config.l1_fee_scalar,
            desired_config.system_config.l1_fee_scalar
        );

        assert_eq!(
            optimism_config.system_config.gas_limit,
            desired_config.system_config.gas_limit
        );

        // Generate Base config and compare with optimism config
        let desired_config = ChainConfig::from_network_name("base");
        assert_ne!(optimism_config.l2_chain_id, desired_config.l2_chain_id);
        assert_ne!(
            optimism_config.deposit_contract,
            desired_config.deposit_contract
        );
    }

    #[test]
    #[should_panic(expected = "Invalid network name")]
    fn test_chain_config_unknown_chain() {
        // Should panic if chain isn't recognized
        _ = ChainConfig::from_network_name("magichain");
    }
}
