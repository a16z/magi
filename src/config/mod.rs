use std::{path::PathBuf, str::FromStr};

use ethers_core::types::{Address, H256};

use crate::common::{BlockInfo, Epoch};

/// A system configuration
#[derive(Debug, Clone)]
pub struct Config {
    /// The base chain RPC URL
    pub l1_rpc: String,
    /// Url of the L2 engine API
    pub engine_url: String,
    /// JWT secret for the L2 engine API
    pub jwt_secret: String,
    /// Location of the database folder
    pub db_location: Option<PathBuf>,
    /// The base chain config
    pub chain: ChainConfig,
}

/// A Chain Configuration
#[derive(Debug, Clone)]
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
    pub sequence_window_size: u64,
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
            sequence_window_size: 120,
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
