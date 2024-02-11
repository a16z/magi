use ethers::types::{Block, Transaction, H256, U256};

use super::BatcherTransactionData;
use crate::{config::SystemConfig, derive::stages::attributes::UserDeposited};

/// Data tied to a specific L1 block
#[derive(Debug)]
pub struct L1Info {
    /// L1 block data
    pub block_info: L1BlockInfo,
    /// The system config at the block
    pub system_config: SystemConfig,
    /// User deposits from that block
    pub user_deposits: Vec<UserDeposited>,
    /// Batcher transactions in block
    pub batcher_transactions: Vec<BatcherTransactionData>,
    /// Whether the block has finalized
    pub finalized: bool,
}

/// L1 block info
#[derive(Debug, Clone)]
pub struct L1BlockInfo {
    /// L1 block number
    pub number: u64,
    /// L1 block hash
    pub hash: H256,
    /// L1 block timestamp
    pub timestamp: u64,
    /// L1 base fee per gas
    pub base_fee: U256,
    /// L1 mix hash (prevrandao)
    pub mix_hash: H256,
    /// Post-Ecotone beacon block root
    pub parent_beacon_block_root: Option<H256>,
}

impl TryFrom<&Block<Transaction>> for L1BlockInfo {
    type Error = eyre::Error;

    fn try_from(value: &Block<Transaction>) -> std::result::Result<Self, Self::Error> {
        let number = value
            .number
            .ok_or(eyre::eyre!("block not included"))?
            .as_u64();

        let hash = value.hash.ok_or(eyre::eyre!("block not included"))?;

        let timestamp = value.timestamp.as_u64();

        let base_fee = value
            .base_fee_per_gas
            .ok_or(eyre::eyre!("block is pre london"))?;

        let mix_hash = value.mix_hash.ok_or(eyre::eyre!("block not included"))?;

        let parent_beacon_block_root = value.parent_beacon_block_root;

        Ok(L1BlockInfo {
            number,
            hash,
            timestamp,
            base_fee,
            mix_hash,
            parent_beacon_block_root,
        })
    }
}
