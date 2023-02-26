use std::str::FromStr;

use ethers_core::types::{Address, H256};

use crate::common::BlockID;

/// A system configuration
#[derive(Debug, Clone)]
pub struct Config {
    /// The base chain RPC URL
    pub l1_rpc: String,
    /// The base chain config
    pub chain: ChainConfig,
    /// The maximum number of intermediate pending channels
    pub max_channels: usize,
    /// The max timeout for a channel (as measured by the frame L1 block number)
    pub max_timeout: u64,
}

/// A Chain Configuration
#[derive(Debug, Clone)]
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
                parent_hash: hash("0xf8b68fcad82739208baff929ef51dff682b5960a57cad693babe01b23fd65460"),
                number: 8300214,
            },
            l2_genesis: BlockID {
                hash: hash("0x0f783549ea4313b784eadd9b8e8a69913b368b7366363ea814d7707ac505175f"),
                number: 4061224,
                parent_hash: hash("0x31267a44f1422f4cab59b076548c075e79bd59e691a23fbce027f572a2a49dc9"),
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
