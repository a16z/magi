use ethers::{
    types::H256,
    utils::rlp::{DecoderError, Rlp},
};

use crate::common::RawTransaction;

use super::block_input::BlockInput;

/// Represents a single batch: a single encoded L2 block
#[derive(Debug, Clone)]
pub struct SingleBatch {
    /// Block hash of the previous L2 block
    pub parent_hash: H256,
    /// The batch epoch number. Same as the first L1 block number in the epoch.
    pub epoch_num: u64,
    /// The block hash of the first L1 block in the epoch
    pub epoch_hash: H256,
    /// The L2 block timestamp of this batch
    pub timestamp: u64,
    /// The L2 block transactions in this batch
    pub transactions: Vec<RawTransaction>,
    /// The L1 block number this batch was fully derived from.
    pub l1_inclusion_block: u64,
}

impl SingleBatch {
    /// Decodes RLP bytes into a [SingleBatch]
    pub fn decode(rlp: &Rlp, l1_inclusion_block: u64) -> Result<Self, DecoderError> {
        let parent_hash = rlp.val_at(0)?;
        let epoch_num = rlp.val_at(1)?;
        let epoch_hash = rlp.val_at(2)?;
        let timestamp = rlp.val_at(3)?;
        let transactions = rlp.list_at(4)?;

        Ok(SingleBatch {
            parent_hash,
            epoch_num,
            epoch_hash,
            timestamp,
            transactions,
            l1_inclusion_block,
        })
    }

    /// If any transactions are empty or deposited transaction types.
    pub fn has_invalid_transactions(&self) -> bool {
        self.transactions
            .iter()
            .any(|tx| tx.0.is_empty() || tx.0[0] == 0x7E)
    }

    /// Returns a [BlockInput] instance for this batch. Represents a single L2 block.
    pub fn block_input(&self) -> BlockInput<u64> {
        BlockInput {
            timestamp: self.timestamp,
            epoch: self.epoch_num,
            transactions: self.transactions.clone(),
            l1_inclusion_block: self.l1_inclusion_block,
        }
    }
}
