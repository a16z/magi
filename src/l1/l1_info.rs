use ethers::types::{Block, Transaction, H256, U256};

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

impl TryFrom<&alloy_rpc_types::Block> for L1BlockInfo {
    type Error = eyre::Error;

    fn try_from(value: &alloy_rpc_types::Block) -> std::result::Result<Self, Self::Error> {
        let number = value
            .header
            .number
            .ok_or(eyre::eyre!("block not included"))?
            .try_into()?;

        let hash = value.header.hash.ok_or(eyre::eyre!("block not included"))?;

        let timestamp = value.header.timestamp.try_into()?;

        let base_fee = value
            .header
            .base_fee_per_gas
            .ok_or(eyre::eyre!("block is pre london"))?;

        let mix_hash = value
            .header
            .mix_hash
            .ok_or(eyre::eyre!("block not included"))?;

        let parent_beacon_block_root = value.header.parent_beacon_block_root;

        Ok(L1BlockInfo {
            number,
            hash: H256::from_slice(&hash.as_slice()),
            timestamp,
            base_fee: base_fee.into(),
            mix_hash: H256::from_slice(&mix_hash.as_slice()),
            parent_beacon_block_root: parent_beacon_block_root
                .map(|x| H256::from_slice(&x.as_slice())),
        })
    }
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
