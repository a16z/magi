use std::sync::{Arc, RwLock};

use eyre::Result;

use crate::{
    common::{Epoch, RawTransaction},
    derive::state::State,
};

pub trait EpochType {}
impl EpochType for u64 {}
impl EpochType for Epoch {}

pub struct BlockInput<E: EpochType> {
    pub timestamp: u64,
    pub epoch: E,
    pub transactions: Vec<RawTransaction>,
    pub l1_inclusion_block: u64,
}

impl BlockInput<u64> {
    pub fn as_full_epoch(self, state: &Arc<RwLock<State>>) -> Result<BlockInput<Epoch>> {
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
