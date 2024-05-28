//! Contains the L1 info types.

use alloy_primitives::{B256, U256};
use alloy_rpc_types::Block;

use crate::{config::SystemConfig, derive::stages::attributes::UserDeposited};

use super::chain_watcher::BatcherTransactionData;

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
    pub hash: B256,
    /// L1 block timestamp
    pub timestamp: u64,
    /// L1 base fee per gas
    pub base_fee: U256,
    /// L1 mix hash (prevrandao)
    pub mix_hash: B256,
    /// Post-Ecotone beacon block root
    pub parent_beacon_block_root: Option<B256>,
}

impl TryFrom<&Block> for L1BlockInfo {
    type Error = anyhow::Error;

    fn try_from(value: &alloy_rpc_types::Block) -> std::result::Result<Self, Self::Error> {
        let number = value
            .header
            .number
            .ok_or(anyhow::anyhow!("block not included"))?;

        let hash = value.header.hash.ok_or(anyhow::anyhow!("block not included"))?;

        let timestamp = value.header.timestamp;

        let base_fee = value
            .header
            .base_fee_per_gas
            .ok_or(anyhow::anyhow!("block is pre london"))?;

        let mix_hash = value
            .header
            .mix_hash
            .ok_or(anyhow::anyhow!("block not included"))?;

        let parent_beacon_block_root = value.header.parent_beacon_block_root;

        Ok(L1BlockInfo {
            number,
            hash,
            timestamp,
            base_fee: U256::from(base_fee),
            mix_hash,
            parent_beacon_block_root,
        })
    }
}
