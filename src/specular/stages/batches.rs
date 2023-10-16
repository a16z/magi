use std::collections::BTreeMap;

use core::fmt::Debug;
use std::cmp::Ordering;
use std::sync::{Arc, RwLock};

use ethers::types::H256;
use eyre::Result;

use crate::common::RawTransaction;
use crate::config::Config;
use crate::derive::stages::batches::Batch;
use crate::derive::state::State;
use crate::derive::PurgeableIterator;
use crate::l1::L1BlockInfo;
use ethers::utils::rlp::{DecoderError, Rlp};

use super::batcher_transactions::SpecularBatcherTransaction;

/// The second stage of Specular's derive pipeline.
/// This stage consumes [SpecularBatcherTransaction]s and produces [SpecularBatchV0]s.
/// One [SpecularBatcherTransaction] may produce multiple [SpecularBatchV0]s.
/// [SpecularBatchV0]s are returned in order of their timestamps.
pub struct SpecularBatches<I> {
    /// Mapping of timestamps to batches
    batches: BTreeMap<u64, SpecularBatchV0>,
    batcher_transaction_iter: I,
    state: Arc<RwLock<State>>,
    config: Arc<Config>,
}

impl<I> Iterator for SpecularBatches<I>
where
    I: Iterator<Item = SpecularBatcherTransaction>,
{
    type Item = Batch;

    fn next(&mut self) -> Option<Self::Item> {
        self.try_next().unwrap_or_else(|_| {
            tracing::debug!("Failed to decode batch");
            None
        })
    }
}

impl<I> PurgeableIterator for SpecularBatches<I>
where
    I: PurgeableIterator<Item = SpecularBatcherTransaction>,
{
    fn purge(&mut self) {
        self.batcher_transaction_iter.purge();
        self.batches.clear();
    }
}

impl<I> SpecularBatches<I> {
    pub fn new(
        batcher_transaction_iter: I,
        state: Arc<RwLock<State>>,
        config: Arc<Config>,
    ) -> Self {
        Self {
            batches: BTreeMap::new(),
            batcher_transaction_iter,
            state,
            config,
        }
    }
}

impl<I> SpecularBatches<I>
where
    I: Iterator<Item = SpecularBatcherTransaction>,
{
    /// This function tries to decode batches from the next [SpecularBatcherTransaction] and
    /// returns the first valid batch if possible.
    fn try_next(&mut self) -> Result<Option<Batch>> {
        let batcher_transaction = self.batcher_transaction_iter.next();
        if let Some(batcher_transaction) = batcher_transaction {
            let batches = decode_batches(&batcher_transaction, &self.state)?;
            batches.into_iter().for_each(|batch| {
                tracing::debug!(
                    "saw batch: t={}, bn={:?}, e={}",
                    batch.timestamp,
                    batch.l2_block_number,
                    batch.l1_inclusion_block,
                );
                self.batches.insert(batch.timestamp, batch);
            });
        }

        let derived_batch = loop {
            if let Some((_, batch)) = self.batches.first_key_value() {
                match self.batch_status(batch) {
                    BatchStatus::Accept => {
                        let batch = batch.clone();
                        self.batches.remove(&batch.timestamp);
                        break Some(batch);
                    }
                    BatchStatus::Drop => {
                        tracing::warn!("dropping invalid batch");
                        let timestamp = batch.timestamp;
                        self.batches.remove(&timestamp);
                    }
                }
            } else {
                break None;
            }
        };

        // TODO[zhe]: handle empty epoch
        let batch = derived_batch;

        Ok(batch.map(|batch| batch.into()))
    }

    /// Determine whether a batch is valid.
    fn batch_status(&self, batch: &SpecularBatchV0) -> BatchStatus {
        let state = self.state.read().unwrap();
        let head = state.safe_head;
        let next_timestamp = head.timestamp + self.config.chain.blocktime;

        // check timestamp range
        // TODO[zhe]: do we need this?
        match batch.timestamp.cmp(&next_timestamp) {
            Ordering::Greater | Ordering::Less => return BatchStatus::Drop,
            Ordering::Equal => (),
        }

        // check that block builds on existing chain
        if batch.l2_block_number != head.number + 1 {
            tracing::warn!("invalid block number");
            return BatchStatus::Drop;
        }

        // TODO[zhe]: check inclusion delay, batch origin epoch, and sequencer drift

        if batch.has_invalid_transactions() {
            tracing::warn!("invalid transaction");
            return BatchStatus::Drop;
        }

        BatchStatus::Accept
    }
}

/// Decode [SpecularBatchV0] from a [SpecularBatcherTransaction].
fn decode_batches(
    batcher_ransaction: &SpecularBatcherTransaction,
    state: &RwLock<State>,
) -> Result<Vec<SpecularBatchV0>> {
    let mut batches = Vec::new();

    let state = state.read().unwrap();
    let l1_info = &state
        .l1_info_by_number(batcher_ransaction.l1_inclusion_block)
        .expect("L1 block must been seen when batcher transaction is decoded")
        .block_info;

    let rlp = Rlp::new(&batcher_ransaction.tx_batch);
    let first_l2_block_number: u64 = rlp.val_at(0)?;
    for (batch, idx) in rlp.at(1)?.iter().zip(0u64..) {
        let batch = SpecularBatchV0::decode(&batch, first_l2_block_number + idx, l1_info)?; // TODO[zhe]: derive l1 inclusion block
        batches.push(batch);
    }

    Ok(batches)
}

#[derive(Debug, Clone, PartialEq)]
enum BatchStatus {
    Drop,
    Accept,
}

/// A batch of transactions with block contexts, which is essentially an L2 block.
#[derive(Debug, Clone)]
pub struct SpecularBatchV0 {
    pub timestamp: u64,
    pub l2_block_number: u64,
    pub transactions: Vec<RawTransaction>,
    pub l1_inclusion_block: u64,
    pub l1_inclusion_hash: H256,
}

impl SpecularBatchV0 {
    fn decode(
        rlp: &Rlp,
        l2_block_number: u64,
        l1_info: &L1BlockInfo,
    ) -> Result<Self, DecoderError> {
        let timestamp = rlp.val_at(0)?;
        let transactions = rlp.list_at(1)?;

        Ok(Self {
            timestamp,
            l2_block_number,
            transactions,
            l1_inclusion_block: l1_info.number,
            l1_inclusion_hash: l1_info.hash,
        })
    }

    fn has_invalid_transactions(&self) -> bool {
        self.transactions.iter().any(|tx| tx.0.is_empty())
    }
}

impl From<SpecularBatchV0> for Batch {
    fn from(val: SpecularBatchV0) -> Self {
        Batch {
            epoch_num: val.l1_inclusion_block, // TODO[zhe]: we simply let the epoch number be the l1 inclusion block number
            epoch_hash: val.l1_inclusion_hash,
            parent_hash: Default::default(), // we don't care about parent hash
            timestamp: val.timestamp,
            transactions: val.transactions,
            l1_inclusion_block: val.l1_inclusion_block,
        }
    }
}
