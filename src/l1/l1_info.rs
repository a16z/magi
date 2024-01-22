use ethers::types::{Address, Block, Transaction, H256, U256};
use eyre::Result;

use crate::{config::SystemConfig, derive::stages::attributes::UserDeposited};

type BatcherTransactionData = Vec<u8>;

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
}

impl L1Info {
    pub fn new(
        block: &Block<Transaction>,
        user_deposits: Vec<UserDeposited>,
        batch_inbox: Address,
        finalized: bool,
        system_config: SystemConfig,
    ) -> Result<Self> {
        let block_number = block
            .number
            .ok_or(eyre::eyre!("block not included"))?
            .as_u64();

        let block_hash = block.hash.ok_or(eyre::eyre!("block not included"))?;

        let block_info = L1BlockInfo {
            number: block_number,
            hash: block_hash,
            timestamp: block.timestamp.as_u64(),
            base_fee: block
                .base_fee_per_gas
                .ok_or(eyre::eyre!("block is pre london"))?,
            mix_hash: block.mix_hash.ok_or(eyre::eyre!("block not included"))?,
        };

        let batcher_transactions =
            create_batcher_transactions(block, system_config.batch_sender, batch_inbox);

        Ok(L1Info {
            block_info,
            system_config,
            user_deposits,
            batcher_transactions,
            finalized,
        })
    }
}

fn create_batcher_transactions(
    block: &Block<Transaction>,
    batch_sender: Address,
    batch_inbox: Address,
) -> Vec<BatcherTransactionData> {
    block
        .transactions
        .iter()
        .filter(|tx| tx.from == batch_sender && tx.to.map(|to| to == batch_inbox).unwrap_or(false))
        .map(|tx| tx.input.to_vec())
        .collect()
}
