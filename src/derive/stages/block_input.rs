use std::sync::{Arc, RwLock};

use eyre::Result;

use crate::{
    common::{Epoch, RawTransaction},
    derive::state::State,
};

/// A marker trait to allow representing an epoch as either a block number or an [Epoch]
pub trait EpochType {}
impl EpochType for u64 {}
impl EpochType for Epoch {}

/// A single L2 block derived from a batch.
#[derive(Debug)]
pub struct BlockInput<E: EpochType> {
    /// Timestamp of the L2 block
    pub timestamp: u64,
    /// The corresponding epoch
    pub epoch: E,
    /// Transactions included in this block
    pub transactions: Vec<RawTransaction>,
    /// The L1 block this batch was fully derived from
    pub l1_inclusion_block: u64,
}

impl BlockInput<u64> {
    /// Returns the Block Input with full [Epoch] details.
    pub fn with_full_epoch(self, state: &Arc<RwLock<State>>) -> Result<BlockInput<Epoch>> {
        let state = state.read().map_err(|_| eyre::eyre!("lock poisoned"))?;
        let epoch = state
            .epoch_by_number(self.epoch)
            .ok_or(eyre::eyre!("epoch not found"))?;

        Ok(BlockInput {
            timestamp: self.timestamp,
            epoch,
            transactions: self.transactions,
            l1_inclusion_block: self.l1_inclusion_block,
        })
    }
}
