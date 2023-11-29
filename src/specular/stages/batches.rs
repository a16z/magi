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
use crate::specular::config::SystemAccounts;
use ethers::{
    types::Transaction,
    utils::rlp::{Decodable, Rlp},
};

use super::batcher_transactions::SpecularBatcherTransaction;
use crate::specular::common::{
    SetL1OracleValuesInput, SET_L1_ORACLE_VALUES_ABI, SET_L1_ORACLE_VALUES_SELECTOR,
};

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
            let batches = decode_batches(&batcher_transaction, &self.state, &self.config)?;
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

        let batch = if derived_batch.is_none() {
            let state = self.state.read().unwrap();

            let current_l1_block = state.current_epoch_num;
            let safe_head = state.safe_head;
            let epoch = state.safe_epoch;
            let next_epoch = state.epoch_by_number(epoch.number + 1);
            let seq_window_size = self.config.chain.seq_window_size;

            if let Some(next_epoch) = next_epoch {
                if current_l1_block > epoch.number + seq_window_size {
                    let next_timestamp = safe_head.timestamp + self.config.chain.blocktime;
                    let epoch = if next_timestamp < next_epoch.timestamp {
                        epoch
                    } else {
                        next_epoch
                    };
                    tracing::trace!(
                        "inserting empty batch | ts={} epoch_num={}",
                        epoch.number,
                        next_timestamp
                    );
                    Some(Batch {
                        epoch_num: epoch.number,
                        epoch_hash: epoch.hash,
                        parent_hash: Default::default(), // We don't care about parent_hash
                        timestamp: next_timestamp,
                        transactions: Vec::new(),
                        l1_inclusion_block: current_l1_block,
                    })
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            derived_batch.map(|batch| batch.into())
        };

        Ok(batch)
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

/// Decode Specular batches from a [SpecularBatcherTransaction] based on its version.
/// Currently only version 0 is supported.
// TODO: consider returning a generic/trait-type to support multiple versions.
fn decode_batches(
    batcher_tx: &SpecularBatcherTransaction,
    state: &RwLock<State>,
    config: &Config,
) -> Result<Vec<SpecularBatchV0>> {
    if batcher_tx.version != 0 {
        eyre::bail!("unsupported batcher transaction version");
    }
    decode_batches_v0(batcher_tx, state, config)
}

/// Decodes [SpecularBatchV0]s from a [SpecularBatcherTransaction].
/// [SpecularBatcherTransaction] contains multiple lists of [SpecularBatchV0]s.
/// For each batch list in [SpecularBatcherTransaction], the first [SpecualrBatchV0] is an epoch update
/// if the first transaction in the batch is a `setL1OracleValues` call.
fn decode_batches_v0(
    batcher_tx: &SpecularBatcherTransaction,
    state: &RwLock<State>,
    config: &Config,
) -> Result<Vec<SpecularBatchV0>> {
    let mut batches = Vec::new();
    let batch_lists = Rlp::new(&batcher_tx.tx_batch);
    // Get l2 safe head info.
    let state = state.read().unwrap();
    let safe_l2_num = state.safe_head.number;
    let safe_l2_ts = state.safe_head.timestamp;
    // Get l2 safe epoch info.
    let mut epoch_num = state.safe_epoch.number;
    let mut epoch_hash = state.safe_epoch.hash;
    // Record the local l2 block number to check for duplicates and missing blocks.
    let mut local_l2_num = safe_l2_num;
    // Decode each batch list in the batcher transaction.
    for batch_list in batch_lists.iter() {
        // Decode the first l2 block number at offset 0.
        let batch_first_l2_num: u64 = batch_list.val_at(0)?;
        // Check for duplicates.
        if batch_first_l2_num < local_l2_num {
            tracing::warn!("invalid batcher transaction: contains already accepted batches | safe_head={} first_l2_block_num={}", local_l2_num, batch_first_l2_num);
            eyre::bail!("invalid batcher transaction: contains already accepted batches");
        }
        // Insert empty batches for missing blocks.
        for i in local_l2_num + 1..batch_first_l2_num {
            let batch = SpecularBatchV0 {
                epoch_num,
                epoch_hash,
                timestamp: (i - safe_l2_num) * config.chain.blocktime + safe_l2_ts,
                transactions: Vec::new(),
                l2_block_number: i,
                l1_inclusion_block: state.current_epoch_num,
                l1_oracle_values: None,
            };
            tracing::trace!(
                "inserting empty batch | num={} ts={}",
                batch.l2_block_number,
                batch.timestamp,
            );
            batches.push(batch);
        }
        // Update the local l2 block number.
        // We're supposed to have inserted empty batches until right before the first batch in the list.
        local_l2_num = batch_first_l2_num - 1;
        let batch_first_l2_ts =
            (batch_first_l2_num - safe_l2_num) * config.chain.blocktime + safe_l2_ts;
        // Decode the transaction batches at offset 1.
        for (batch, idx) in batch_list.at(1)?.iter().zip(0u64..) {
            let transactions: Vec<RawTransaction> = batch.as_list()?;
            // Try decode the `setL1OacleValues` call if it is the first batch in the list.
            let l1_oracle_values = if idx == 0 {
                transactions.first().and_then(try_decode_l1_oracle_values)
            } else {
                None
            };
            // Update the local epoch info if there's a new epoch.
            if let Some((new_epoch_num, _, _, new_epoch_hash, _)) = l1_oracle_values {
                epoch_num = new_epoch_num.as_u64();
                epoch_hash = new_epoch_hash;
            }
            // Create the batch.
            let batch = SpecularBatchV0 {
                epoch_num,
                epoch_hash,
                timestamp: batch_first_l2_ts + idx * config.chain.blocktime,
                transactions,
                l2_block_number: batch_first_l2_num + idx,
                l1_inclusion_block: batcher_tx.l1_inclusion_block,
                l1_oracle_values,
            };
            batches.push(batch);
            // Update the local l2 block number.
            local_l2_num += 1;
        }
    }

    Ok(batches)
}

#[derive(Debug, Clone, PartialEq)]
enum BatchStatus {
    Drop,
    Accept,
}

/// A batch of transactions, along with payload attributes.
#[derive(Debug, Clone)]
pub struct SpecularBatchV0 {
    pub epoch_num: u64,
    pub epoch_hash: H256,
    pub timestamp: u64,
    pub l2_block_number: u64,
    pub transactions: Vec<RawTransaction>,
    pub l1_inclusion_block: u64,
    pub l1_oracle_values: Option<SetL1OracleValuesInput>,
}

impl SpecularBatchV0 {
    fn has_invalid_transactions(&self) -> bool {
        self.transactions.iter().any(|tx| tx.0.is_empty())
    }
}

impl From<SpecularBatchV0> for Batch {
    fn from(val: SpecularBatchV0) -> Self {
        Batch {
            epoch_num: val.epoch_num,
            epoch_hash: val.epoch_hash,
            parent_hash: Default::default(), // not used
            timestamp: val.timestamp,
            transactions: val.transactions,
            l1_inclusion_block: val.l1_inclusion_block,
        }
    }
}

fn try_decode_l1_oracle_values(tx: &RawTransaction) -> Option<SetL1OracleValuesInput> {
    let tx = Transaction::decode(&Rlp::new(&tx.0)).ok()?;
    if tx.to? != SystemAccounts::default().l1_oracle {
        return None;
    }

    let input = SET_L1_ORACLE_VALUES_ABI
        .decode_with_selector(*SET_L1_ORACLE_VALUES_SELECTOR, &tx.input.0)
        .ok()?;

    Some(input)
}
