use std::{fmt::Debug, fs, iter, net::SocketAddr, path::Path, str::FromStr};

use discv5::enr::{CombinedKey, Enr};
use ethers::types::{Address, H256, U256};
use eyre::{anyhow, ensure, Result};
use figment::{
    providers::{Format, Serialized, Toml},
    Figment, Provider,
};
use libp2p_identity::secp256k1::SecretKey;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    network,
    types::common::{BlockInfo, Epoch},
};

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

/// Sequencer config.
/// The tuple is maximum lag between safe L2 block (confirmed by L1) and new block.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct SequencerConfig(u64);

impl SequencerConfig {
    pub fn new(max_safe_lag: u64) -> Self {
        SequencerConfig(max_safe_lag)
    }

    pub fn max_safe_lag(&self) -> u64 {
        self.0
    }
}

/// A system configuration. Can be built by combining different sources using `ConfigBuilder`.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    #[serde(skip_serializing_if = "String::is_empty")]
    pub jwt_secret: String,

    /// A trusted L2 RPC URL to use for fast/checkpoint syncing
    pub checkpoint_sync_url: Option<String>,

    /// The port of RPC server
    pub rpc_port: u16,

    /// The devnet mode
    pub devnet: bool,

    /// The sequencer config.
    pub sequencer: Option<SequencerConfig>,

    /// P2P listening address
    pub p2p_listen: SocketAddr,

    // Secret key Secp256k1 for P2P.
    #[serde(
        default,
        serialize_with = "serialize_secret_key",
        deserialize_with = "deserialize_secret_key",
        skip_serializing_if = "Option::is_none"
    )]
    pub p2p_secret_key: Option<SecretKey>,

    /// Bootnodes to which you need to connect initially.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p2p_bootnodes: Option<Vec<Enr<CombinedKey>>>,

    #[serde(
        default,
        serialize_with = "serialize_secret_key",
        deserialize_with = "deserialize_secret_key",
        skip_serializing_if = "Option::is_none"
    )]
    pub p2p_sequencer_secret_key: Option<SecretKey>,
}

impl Config {
    pub fn save(&self, path: impl AsRef<Path> + Debug) -> Result<()> {
        if path.as_ref().exists() {
            ensure!(
                path.as_ref().is_file(),
                "An incorrect configuration path {path:?} was passed"
            );
        } else {
            let dir = path.as_ref()
                .parent()
                .ok_or(anyhow!("An incorrect configuration path {path:?} was passed. Only the filename was specified."))?;
            if !dir.exists() {
                fs::create_dir_all(dir)?;
            }
        }

        let config_as_string = toml::to_string(self)?;

        fs::write(path, config_as_string)?;
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            l1_rpc_url: "http://127.0.0.1:8545".to_string(),
            l2_rpc_url: "http://127.0.0.1:9545".to_string(),
            l2_engine_url: "http://127.0.0.1:9551".to_string(),
            chain: ChainConfig::optimism(),
            jwt_secret: Default::default(),
            checkpoint_sync_url: None,
            rpc_port: 7545,
            devnet: false,
            sequencer: None,
            p2p_listen: *network::LISTENING,
            p2p_secret_key: None,
            p2p_bootnodes: None,
            p2p_sequencer_secret_key: None,
        }
    }
}

pub struct ConfigBuilder {
    figment: Figment,
}

/// System configuration builder
impl ConfigBuilder {
    pub fn defaults(self, defaults: impl Serialize) -> Self {
        self.merge(Serialized::defaults(defaults))
    }

    pub fn chain(self, chain: ChainConfig) -> Self {
        let chain_provider: Serialized<ChainProvider> = chain.into();
        self.merge(chain_provider)
    }

    pub fn toml(self, toml_path: impl AsRef<Path> + Debug) -> Self {
        let mut toml_provider = Toml::file(&toml_path);
        if !toml_path.as_ref().exists() {
            toml_provider = toml_provider.nested();
        }
        self.toml_internal(toml_provider)
    }

    pub fn cli(self, cli_config: CliConfig) -> Self {
        self.merge(Serialized::defaults(cli_config))
    }

    pub fn build(self) -> Config {
        self.figment.extract().unwrap_or_else(|err| {
                match err.kind {
                    figment::error::Kind::MissingField(field) => {
                        let field = field.replace('_', "-");
                        eprintln!("\x1b[91merror\x1b[0m: missing configuration field: {field}");
                        eprintln!("\n\ttry supplying the proper command line argument: --{field}");
                        eprintln!("\talternatively, you can add the field to your magi.toml file or as an environment variable");
                        eprintln!("\nfor more information, check the github README");
                    }
                    _ => eprintln!("cannot parse configuration: {err}"),
                }
                std::process::exit(1);
            })
    }

    fn toml_internal(self, toml_provider: figment::providers::Data<Toml>) -> Self {
        self.merge(toml_provider)
    }

    fn merge(self, provider: impl Provider) -> Self {
        Self {
            figment: self.figment.merge(provider),
        }
    }
}

impl Default for ConfigBuilder {
    fn default() -> Self {
        Self {
            figment: Figment::new().merge(Serialized::defaults(Config::default())),
        }
    }
}

pub fn serialize_secret_key<S>(secret_key: &Option<SecretKey>, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let secret_key_bytes = secret_key.as_ref().map(|v| v.to_bytes());
    let secret_key_hex = secret_key_bytes.map(hex::encode);
    s.serialize_some(&secret_key_hex)
}

pub fn deserialize_secret_key<'de, D>(deserializer: D) -> Result<Option<SecretKey>, D::Error>
where
    D: Deserializer<'de>,
{
    let hex_key: Option<String> = Option::deserialize(deserializer)?;
    let secret_key = match hex_key {
        Some(hex_string) => {
            Some(secret_key_from_hex(&hex_string).map_err(serde::de::Error::custom)?)
        }
        None => None,
    };

    Ok(secret_key)
}

pub fn secret_key_from_hex(value: &str) -> eyre::Result<SecretKey> {
    let bytes: [u8; 32] = hex::decode(value)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(anyhow!("Invalid private key passed"))?;

    let secret_key = SecretKey::try_from_bytes(bytes)?;
    Ok(secret_key)
}

pub fn serialize_u256_with_leading_zeroes<S>(value: &U256, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut buf = [0; 32];
    value.to_big_endian(&mut buf);
    s.serialize_some(&H256::from_slice(&buf))
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
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub devnet: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequencer: Option<SequencerConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rpc_port: Option<u16>,

    /// Secret key Secp256k1 for P2P.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_secret_key",
        deserialize_with = "deserialize_secret_key"
    )]
    pub p2p_secret_key: Option<SecretKey>,

    /// P2P listening address
    pub p2p_listen: SocketAddr,

    /// Bootnodes to which you need to connect initially
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p2p_bootnodes: Option<Vec<Enr<CombinedKey>>>,

    /// Secret key Secp256k1 for Sequencer.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_secret_key",
        deserialize_with = "deserialize_secret_key"
    )]
    pub p2p_sequencer_secret_key: Option<SecretKey>,
}

/// A Chain Configuration
///
/// This structure is also used to parse external chain configs from JSON.
/// This interface extends the default output of the `op-node` genesis devnet
/// setup command `--outfile.rollup` flag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainConfig {
    /// The network name
    #[serde(default = "default_network")]
    pub network: String,
    /// The L1 chain id
    pub l1_chain_id: u64,
    /// The L2 chain id
    pub l2_chain_id: u64,
    /// Genesis configuration
    pub genesis: GenesisInfo,
    /// Network block time
    #[serde(default = "default_block_time")]
    pub block_time: u64,
    /// Maximum timestamp drift
    pub max_sequencer_drift: u64,
    /// Number of L1 blocks in a sequence window
    pub seq_window_size: u64,
    /// The maximum byte size of all pending channels
    #[serde(default = "default_max_channel_size")]
    pub max_channel_size: u64,
    /// The max timeout for a channel (as measured by the frame L1 block number)
    pub channel_timeout: u64,
    /// Timestamp of the regolith hardfork
    pub regolith_time: u64,
    /// Timestamp of the canyon hardfork
    pub canyon_time: u64,
    /// The batch inbox address
    pub batch_inbox_address: Address,
    /// The deposit contract address
    pub deposit_contract_address: Address,
    /// The L1 system config contract
    pub l1_system_config_address: Address,
    /// L2 To L1 Message passer address
    #[serde(default = "default_l2_to_l1_message_passer")]
    pub l2_to_l1_message_passer: Address,
}

impl TryFrom<&str> for ChainConfig {
    type Error = eyre::Report;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let chain = match value {
            "optimism" => ChainConfig::optimism(),
            "optimism-goerli" => ChainConfig::optimism_goerli(),
            "optimism-sepolia" => ChainConfig::optimism_sepolia(),
            "base" => ChainConfig::base(),
            "base-goerli" => ChainConfig::base_goerli(),
            file if file.ends_with(".json") => ChainConfig::from_json(file),
            _ => eyre::bail!(
                "Invalid network name. \\
                Please use one of the following: 'optimism', 'optimism-goerli', 'base-goerli', `optimism-sepolia`, `base`, `base-goerli`  \\
                You can also use a JSON file path for custom configuration."
            ),
        };

        Ok(chain)
    }
}

/// Optimism system config contract values
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemConfig {
    /// Batch sender address
    pub batcher_addr: Address,
    /// L2 gas limit
    pub gas_limit: u64,
    /// L1 fee overhead
    #[serde(serialize_with = "serialize_u256_with_leading_zeroes")]
    pub overhead: U256,
    /// L1 fee scalar
    #[serde(serialize_with = "serialize_u256_with_leading_zeroes")]
    pub scalar: U256,
    /// Sequencer's signer for unsafe blocks
    #[serde(default)]
    pub unsafe_block_signer: Address,
}

impl SystemConfig {
    /// Encoded batch sender as a H256
    pub fn batcher_hash(&self) -> H256 {
        let mut batch_sender_bytes = self.batcher_addr.as_bytes().to_vec();
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

impl ChainConfig {
    /// Read and parse a chain config object from a JSON file path
    pub fn from_json(path: &str) -> Self {
        let file = std::fs::File::open(path).unwrap();
        serde_json::from_reader(file).unwrap()
    }

    pub fn optimism() -> Self {
        Self {
            network: "optimism".to_string(),
            l1_chain_id: 1,
            l2_chain_id: 10,
            genesis: GenesisInfo {
                l1: ChainGenesisInfo {
                    hash: hash(
                        "0x438335a20d98863a4c0c97999eb2481921ccd28553eac6f913af7c12aec04108",
                    ),
                    number: 17422590,
                    parent_hash: H256::zero(),
                },
                l2: ChainGenesisInfo {
                    hash: hash(
                        "0xdbf6a80fef073de06add9b0d14026d6e5a86c85f6d102c36d3d8e9cf89c2afd3",
                    ),
                    number: 105235063,
                    parent_hash: hash(
                        "0x21a168dfa5e727926063a28ba16fd5ee84c814e847c81a699c7a0ea551e4ca50",
                    ),
                },
                l2_time: 1686068903,
                system_config: SystemConfig {
                    batcher_addr: addr("0x6887246668a3b87f54deb3b94ba47a6f63f32985"),
                    gas_limit: 30_000_000,
                    overhead: U256::from(188),
                    scalar: U256::from(684000),
                    unsafe_block_signer: addr("0xAAAA45d9549EDA09E70937013520214382Ffc4A2"),
                },
            },
            batch_inbox_address: addr("0xff00000000000000000000000000000000000010"),
            deposit_contract_address: addr("0xbEb5Fc579115071764c7423A4f12eDde41f106Ed"),
            l1_system_config_address: addr("0x229047fed2591dbec1eF1118d64F7aF3dB9EB290"),
            l2_to_l1_message_passer: addr("0x4200000000000000000000000000000000000016"),
            max_channel_size: 100_000_000,
            channel_timeout: 300,
            seq_window_size: 3600,
            max_sequencer_drift: 600,
            block_time: 2,
            regolith_time: 0,
            canyon_time: u64::MAX,
        }
    }

    pub fn optimism_goerli() -> Self {
        Self {
            network: "optimism-goerli".to_string(),
            l1_chain_id: 5,
            l2_chain_id: 420,
            genesis: GenesisInfo {
                l1: ChainGenesisInfo {
                    hash: hash(
                        "0x6ffc1bf3754c01f6bb9fe057c1578b87a8571ce2e9be5ca14bace6eccfd336c7",
                    ),
                    number: 8300214,
                    parent_hash: H256::zero(),
                },
                l2: ChainGenesisInfo {
                    hash: hash(
                        "0x0f783549ea4313b784eadd9b8e8a69913b368b7366363ea814d7707ac505175f",
                    ),
                    number: 4061224,
                    parent_hash: hash(
                        "0x31267a44f1422f4cab59b076548c075e79bd59e691a23fbce027f572a2a49dc9",
                    ),
                },
                l2_time: 1673550516,
                system_config: SystemConfig {
                    batcher_addr: addr("0x7431310e026b69bfc676c0013e12a1a11411eec9"),
                    gas_limit: 25_000_000,
                    overhead: U256::from(2100),
                    scalar: U256::from(1000000),
                    unsafe_block_signer: addr("0x715b7219D986641DF9eFd9C7Ef01218D528e19ec"),
                },
            },
            l1_system_config_address: addr("0xAe851f927Ee40dE99aaBb7461C00f9622ab91d60"),
            batch_inbox_address: addr("0xff00000000000000000000000000000000000420"),
            deposit_contract_address: addr("0x5b47E1A08Ea6d985D6649300584e6722Ec4B1383"),
            l2_to_l1_message_passer: addr("0xEF2ec5A5465f075E010BE70966a8667c94BCe15a"),
            max_channel_size: 100_000_000,
            channel_timeout: 300,
            seq_window_size: 3600,
            max_sequencer_drift: 600,
            regolith_time: 1679079600,
            canyon_time: 1699981200,
            block_time: 2,
        }
    }

    pub fn optimism_sepolia() -> Self {
        Self {
            network: "optimism-sepolia".to_string(),
            l1_chain_id: 11155111,
            l2_chain_id: 11155420,
            genesis: GenesisInfo {
                l1: ChainGenesisInfo {
                    hash: hash(
                        "0x48f520cf4ddaf34c8336e6e490632ea3cf1e5e93b0b2bc6e917557e31845371b",
                    ),
                    number: 4071408,
                    parent_hash: H256::zero(),
                },
                l2: ChainGenesisInfo {
                    hash: hash(
                        "0x102de6ffb001480cc9b8b548fd05c34cd4f46ae4aa91759393db90ea0409887d",
                    ),
                    number: 0,
                    parent_hash: hash(
                        "0x0000000000000000000000000000000000000000000000000000000000000000",
                    ),
                },
                l2_time: 1691802540,
                system_config: SystemConfig {
                    batcher_addr: addr("0x8F23BB38F531600e5d8FDDaAEC41F13FaB46E98c"),
                    gas_limit: 30_000_000,
                    overhead: U256::from(188),
                    scalar: U256::from(684000),
                    unsafe_block_signer: addr("0x57CACBB0d30b01eb2462e5dC940c161aff3230D3"),
                },
            },
            l1_system_config_address: addr("0x034edd2a225f7f429a63e0f1d2084b9e0a93b538"),
            batch_inbox_address: addr("0xff00000000000000000000000000000011155420"),
            deposit_contract_address: addr("0x16fc5058f25648194471939df75cf27a2fdc48bc"),
            l2_to_l1_message_passer: addr("0x4200000000000000000000000000000000000016"),
            max_channel_size: 100_000_000,
            channel_timeout: 300,
            seq_window_size: 3600,
            max_sequencer_drift: 600,
            regolith_time: 0,
            canyon_time: 1699981200,
            block_time: 2,
        }
    }

    pub fn base() -> Self {
        Self {
            network: "base".to_string(),
            l1_chain_id: 1,
            l2_chain_id: 8453,
            genesis: GenesisInfo {
                l1: ChainGenesisInfo {
                    number: 17481768,
                    hash: hash(
                        "0x5c13d307623a926cd31415036c8b7fa14572f9dac64528e857a470511fc30771",
                    ),
                    parent_hash: H256::zero(),
                },
                l2: ChainGenesisInfo {
                    hash: hash(
                        "0xf712aa9241cc24369b143cf6dce85f0902a9731e70d66818a3a5845b296c73dd",
                    ),
                    number: 0,
                    parent_hash: H256::zero(),
                },
                l2_time: 1686789347,
                system_config: SystemConfig {
                    batcher_addr: addr("0x5050f69a9786f081509234f1a7f4684b5e5b76c9"),
                    gas_limit: 30000000,
                    overhead: U256::from(188),
                    scalar: U256::from(684000),
                    unsafe_block_signer: addr("0xAf6E19BE0F9cE7f8afd49a1824851023A8249e8a"),
                },
            },
            batch_inbox_address: addr("0xff00000000000000000000000000000000008453"),
            deposit_contract_address: addr("0x49048044d57e1c92a77f79988d21fa8faf74e97e"),
            l1_system_config_address: addr("0x73a79fab69143498ed3712e519a88a918e1f4072"),
            l2_to_l1_message_passer: addr("0x4200000000000000000000000000000000000016"),
            max_channel_size: 100_000_000,
            channel_timeout: 300,
            seq_window_size: 3600,
            max_sequencer_drift: 600,
            block_time: 2,
            regolith_time: 0,
            canyon_time: u64::MAX,
        }
    }

    pub fn base_goerli() -> Self {
        Self {
            network: "base-goerli".to_string(),
            l1_chain_id: 5,
            l2_chain_id: 84531,
            genesis: GenesisInfo {
                l1: ChainGenesisInfo {
                    number: 8410981,
                    hash: hash(
                        "0x73d89754a1e0387b89520d989d3be9c37c1f32495a88faf1ea05c61121ab0d19",
                    ),
                    parent_hash: H256::zero(),
                },
                l2: ChainGenesisInfo {
                    hash: hash(
                        "0xa3ab140f15ea7f7443a4702da64c10314eb04d488e72974e02e2d728096b4f76",
                    ),
                    number: 0,
                    parent_hash: H256::zero(),
                },
                l2_time: 1675193616,
                system_config: SystemConfig {
                    batcher_addr: addr("0x2d679b567db6187c0c8323fa982cfb88b74dbcc7"),
                    gas_limit: 25_000_000,
                    overhead: U256::from(2100),
                    scalar: U256::from(1000000),
                    unsafe_block_signer: addr("0x32a4e99A72c11E9DD3dC159909a2D7BD86C1Bc51"),
                },
            },
            l1_system_config_address: addr("0xb15eea247ece011c68a614e4a77ad648ff495bc1"),
            batch_inbox_address: addr("0x8453100000000000000000000000000000000000"),
            deposit_contract_address: addr("0xe93c8cd0d409341205a592f8c4ac1a5fe5585cfa"),
            l2_to_l1_message_passer: addr("0x4200000000000000000000000000000000000016"),
            max_channel_size: 100_000_000,
            channel_timeout: 300,
            seq_window_size: 3600,
            max_sequencer_drift: 600,
            regolith_time: 1683219600,
            canyon_time: 1699981200,
            block_time: 2,
        }
    }

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
            blocktime: 2,
        }
    }

    pub fn l1_start_epoch(&self) -> Epoch {
        Epoch {
            number: self.genesis.l1.number,
            hash: self.genesis.l1.hash,
            timestamp: self.genesis.l2_time,
        }
    }

    pub fn l2_genesis(&self) -> BlockInfo {
        BlockInfo {
            hash: self.genesis.l2.hash,
            number: self.genesis.l2.number,
            parent_hash: self.genesis.l2.parent_hash,
            timestamp: self.genesis.l2_time,
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

fn default_network() -> String {
    "external".to_owned()
}

fn default_block_time() -> u64 {
    2
}

fn default_max_channel_size() -> u64 {
    100_000_000
}

fn default_l2_to_l1_message_passer() -> Address {
    addr("0x4200000000000000000000000000000000000016")
}

/// Genesis configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenesisInfo {
    /// L1 genesis configuration
    pub l1: ChainGenesisInfo,
    /// L2 genesis configuration
    pub l2: ChainGenesisInfo,
    /// L2 genesis timestamp
    pub l2_time: u64,
    /// The initial system config value
    pub system_config: SystemConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainGenesisInfo {
    pub hash: H256,
    pub number: u64,
    #[serde(default)]
    pub parent_hash: H256,
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
            "canyon_time": 0,
            "batch_inbox_address": "0xff00000000000000000000000000000000000000",
            "deposit_contract_address": "0x6900000000000000000000000000000000000001",
            "l1_system_config_address": "0x6900000000000000000000000000000000000009"
          }          
        "#;

        let chain: ChainConfig = serde_json::from_str(devnet_json).unwrap();

        assert_eq!(chain.network, "external");
        assert_eq!(chain.l1_chain_id, 900);
        assert_eq!(chain.l2_chain_id, 901);
        assert_eq!(chain.genesis.l1.number, 0);
        assert_eq!(
            chain.genesis.l1.hash,
            hash("0xdb52a58e7341447d1a9525d248ea07dbca7dfa0e105721dee1aa5a86163c088d")
        );
        assert_eq!(chain.genesis.l2.number, 0);
        assert_eq!(
            chain.genesis.l2.hash,
            hash("0xf85bca315a08237644b06a8350cda3bc0de1593745a91be93daeadb28fb3a32e")
        );
        assert_eq!(chain.genesis.system_config.gas_limit, 30_000_000);
        assert_eq!(chain.genesis.system_config.overhead, U256::from(2100));
        assert_eq!(chain.genesis.system_config.scalar, U256::from(1_000_000));
        assert_eq!(
            chain.genesis.system_config.batcher_addr,
            addr("0x3c44cdddb6a900fa2b585dd299e03d12fa4293bc")
        );
        assert_eq!(
            chain.batch_inbox_address,
            addr("0xff00000000000000000000000000000000000000")
        );
        assert_eq!(
            chain.deposit_contract_address,
            addr("0x6900000000000000000000000000000000000001")
        );
        assert_eq!(
            chain.l1_system_config_address,
            addr("0x6900000000000000000000000000000000000009")
        );
        assert_eq!(chain.max_channel_size, 100_000_000);
        assert_eq!(chain.channel_timeout, 120);
        assert_eq!(chain.seq_window_size, 200);
        assert_eq!(chain.max_sequencer_drift, 300);
        assert_eq!(chain.regolith_time, 0);
        assert_eq!(chain.canyon_time, 0);
        assert_eq!(chain.block_time, 2);
        assert_eq!(
            chain.l2_to_l1_message_passer,
            addr("0x4200000000000000000000000000000000000016")
        );
    }

    #[test]
    fn test_write_chain_config_to_json() -> Result<()> {
        let chain = ChainConfig::optimism();
        let json = serde_json::to_string(&chain)?;

        let expected_json: String = r#"{
          "network": "optimism",
          "l1_chain_id": 1,
          "l2_chain_id": 10,
          "genesis": {
            "l1": {
              "hash": "0x438335a20d98863a4c0c97999eb2481921ccd28553eac6f913af7c12aec04108",
              "number": 17422590,
              "parentHash": "0x0000000000000000000000000000000000000000000000000000000000000000"
            },
            "l2": {
              "hash": "0xdbf6a80fef073de06add9b0d14026d6e5a86c85f6d102c36d3d8e9cf89c2afd3",
              "number": 105235063,
              "parentHash": "0x21a168dfa5e727926063a28ba16fd5ee84c814e847c81a699c7a0ea551e4ca50"
            },
            "l2_time": 1686068903,
            "system_config": {
              "batcherAddr": "0x6887246668a3b87f54deb3b94ba47a6f63f32985",
              "gasLimit": 30000000,
              "overhead": "0x00000000000000000000000000000000000000000000000000000000000000bc",
              "scalar": "0x00000000000000000000000000000000000000000000000000000000000a6fe0",
              "unsafeBlockSigner": "0xaaaa45d9549eda09e70937013520214382ffc4a2"
            }
          },
          "block_time": 2,
          "max_sequencer_drift": 600,
          "seq_window_size": 3600,
          "max_channel_size": 100000000,
          "channel_timeout": 300,
          "regolith_time": 0,
          "batch_inbox_address": "0xff00000000000000000000000000000000000010",
          "deposit_contract_address": "0xbeb5fc579115071764c7423a4f12edde41f106ed",
          "l1_system_config_address": "0x229047fed2591dbec1ef1118d64f7af3db9eb290",
          "l2_to_l1_message_passer": "0x4200000000000000000000000000000000000016"
        }"#
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();

        assert_eq!(json, expected_json);

        Ok(())
    }

    #[test]
    fn test_config_save_load() -> Result<()> {
        // Fill by any data different from default values:
        let config = Config {
            l1_rpc_url: "http://localhost:8888".to_string(),
            l2_rpc_url: "http://127.0.0.1:9999".to_string(),
            l2_engine_url: "http://localhost:5545".to_string(),
            chain: ChainConfig::optimism_sepolia(),
            jwt_secret: "TestApiKey".to_owned(),
            checkpoint_sync_url: Some("http://10.0.0.1:5432".to_string()),
            rpc_port: 123,
            devnet: true,
            sequencer: Some(SequencerConfig(54321)),
            p2p_listen: "10.0.0.1:4444".parse()?,
            p2p_secret_key: Some(SecretKey::generate()),
            p2p_bootnodes: Some(vec![]),
            p2p_sequencer_secret_key: Some(SecretKey::generate()),
        };

        let tempfile = tempfile::NamedTempFile::new()?;

        config.save(tempfile.path())?;

        let config_read = ConfigBuilder::default().toml(tempfile.path()).build();

        assert_eq!(
            serde_json::to_string(&config_read)?,
            serde_json::to_string(&config)?,
            "`config_read` doesn't match to `config`"
        );

        Ok(())
    }

    #[test]
    fn test_cli_config_apply_to_toml() -> Result<()> {
        // Just test data, which are different from defaults:
        const TOML: &str = r#"
            l1_rpc_url = "http://localhost:8545"
            l2_rpc_url = "http://localhost:9545"
            l2_engine_url = "http://localhost:8551"
            jwt_secret = "TestApiKey"
            rpc_port = 7546
            devnet = true
            p2p_listen = "127.0.0.1:9876"

            [chain]
            network = "optimism_goerli"
            l1_chain_id = 11
            l2_chain_id = 12
            block_time = 2
            max_sequencer_drift = 600
            seq_window_size = 3600
            max_channel_size = 100000000
            channel_timeout = 300
            regolith_time = 0
            batch_inbox_address = "0xff00000000000000000000000000000000000010"
            deposit_contract_address = "0xbeb5fc579115071764c7423a4f12edde41f106ed"
            l1_system_config_address = "0x229047fed2591dbec1ef1118d64f7af3db9eb290"
            l2_to_l1_message_passer = "0x4200000000000000000000000000000000000016"

            [chain.genesis]
            l2_time = 1686068905

            [chain.genesis.l1]
            hash = "0x438335a20d98863a4c0c97999eb2481921ccd28553eac6f913af7c12aec04108"
            number = 17422590
            parentHash = "0x0000000000000000000000000000000000000000000000000000000000000000"

            [chain.genesis.l2]
            hash = "0xdbf6a80fef073de06add9b0d14026d6e5a86c85f6d102c36d3d8e9cf89c2afd3"
            number = 105235063
            parentHash = "0x21a168dfa5e727926063a28ba16fd5ee84c814e847c81a699c7a0ea551e4ca50"

            [chain.genesis.system_config]
            batcherAddr = "0x6887246668a3b87f54deb3b94ba47a6f63f32985"
            gasLimit = 30000000
            overhead = "0xbc"
            scalar = "0xa6fe0"
            unsafeBlockSigner = "0xaaaa45d9549eda09e70937013520214382ffc4a2"
        "#;

        let cli_config = CliConfig {
            l1_rpc_url: Some("http://10.0.0.1:3344".to_string()),
            l2_rpc_url: Some("http://10.0.0.1:4455".to_string()),
            l2_engine_url: None,
            jwt_secret: Some("new secret".to_string()),
            checkpoint_sync_url: None,
            devnet: false,
            sequencer: None,
            rpc_port: Some(5555),
            p2p_secret_key: None,
            p2p_listen: *network::LISTENING,
            p2p_bootnodes: None,
            p2p_sequencer_secret_key: None,
        };

        let mut config_expected = ConfigBuilder::default()
            .toml_internal(Toml::string(TOML))
            .build();

        // Copy only values, overridden by `CliConfig`:
        config_expected.l1_rpc_url = cli_config.l1_rpc_url.as_ref().unwrap().clone();
        config_expected.l2_rpc_url = cli_config.l2_rpc_url.as_ref().unwrap().clone();
        config_expected.jwt_secret = cli_config.jwt_secret.as_ref().unwrap().clone();
        config_expected.rpc_port = cli_config.rpc_port.unwrap();
        config_expected.p2p_listen = cli_config.p2p_listen;

        let config = ConfigBuilder::default()
            .toml_internal(Toml::string(TOML))
            .cli(cli_config)
            .build();

        assert_eq!(
            serde_json::to_string(&config)?,
            serde_json::to_string(&config_expected)?,
            "`config` doesn't match to `config_expected`"
        );

        Ok(())
    }
}
