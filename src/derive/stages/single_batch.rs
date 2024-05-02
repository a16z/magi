//! Module to handle [SingleBatch] processing.

use alloy_primitives::B256;
use alloy_rlp::{RlpDecodable, RlpEncodable};

use crate::common::RawTransaction;

use super::block_input::BlockInput;

/// Represents a single batch: a single encoded L2 block
#[derive(Debug, RlpEncodable, RlpDecodable, Clone)]
#[rlp(trailing)]
pub struct SingleBatch {
    /// Block hash of the previous L2 block
    pub parent_hash: B256,
    /// The batch epoch number. Same as the first L1 block number in the epoch.
    pub epoch_num: u64,
    /// The block hash of the first L1 block in the epoch
    pub epoch_hash: B256,
    /// The L2 block timestamp of this batch
    pub timestamp: u64,
    /// The L2 block transactions in this batch
    pub transactions: Vec<RawTransaction>,
    /// The L1 block number this batch was fully derived from.
    pub l1_inclusion_block: Option<u64>,
}

impl SingleBatch {
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
            l1_inclusion_block: self.l1_inclusion_block.unwrap_or(0),
        }
    }
}
