use crate::types::attributes::RawTransaction;
use ethers::{
    types::H256,
    utils::rlp::{DecoderError, Rlp},
};

use super::block_input::BlockInput;

#[derive(Debug, Clone)]
pub struct SingleBatch {
    pub parent_hash: H256,
    pub epoch_num: u64,
    pub epoch_hash: H256,
    pub timestamp: u64,
    pub transactions: Vec<RawTransaction>,
    pub l1_inclusion_block: u64,
}

impl SingleBatch {
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

    pub fn has_invalid_transactions(&self) -> bool {
        self.transactions
            .iter()
            .any(|tx| tx.0.is_empty() || tx.0[0] == 0x7E)
    }

    pub fn block_input(&self) -> BlockInput<u64> {
        BlockInput {
            timestamp: self.timestamp,
            epoch: self.epoch_num,
            transactions: self.transactions.clone(),
            l1_inclusion_block: self.l1_inclusion_block,
        }
    }
}
