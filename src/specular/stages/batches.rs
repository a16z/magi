use std::collections::BTreeMap;

use core::fmt::Debug;
use std::cmp::Ordering;
use std::sync::{Arc, RwLock};

use ethers::types::H256;
use ethers::utils::rlp::Rlp;
use eyre::Result;

use crate::common::RawTransaction;
use crate::config::Config;
use crate::derive::stages::batches::Batch;
use crate::derive::state::State;
use crate::derive::PurgeableIterator;

use super::batcher_transactions::SpecularBatcherTransaction;
use crate::specular::common::SetL1OracleValuesInput;

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

        // check the inclusion delay
        if batch.epoch_num + self.config.chain.seq_window_size < batch.l1_inclusion_block {
            tracing::warn!("inclusion window elapsed");
            return BatchStatus::Drop;
        }

        // TODO[zhe]: check origin epoch and sequencer drift

        // check L1 oracle update transaction
        if batch.l1_oracle_values.is_some() {
            if let Err(err) = check_epoch_update_batch(batch, &state) {
                tracing::warn!("invalid epoch update batch, err={:?}", err);
                return BatchStatus::Drop;
            }
        }

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
                transactions
                    .first()
                    .and_then(|tx| SetL1OracleValuesInput::try_from(tx).ok())
            } else {
                None
            };
            // Update the local epoch info if there's a new epoch.
            if let Some((new_epoch_num, _, _, new_epoch_hash, _, _, _)) = l1_oracle_values {
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

fn check_epoch_update_batch(batch: &SpecularBatchV0, state: &State) -> Result<()> {
    let (epoch_num, timestamp, base_fee, epoch_hash, state_root, _, _) =
        batch.l1_oracle_values.unwrap();
    let target_epoch = state
        .l1_info_by_number(epoch_num.as_u64())
        .ok_or(eyre::eyre!("epoch {} does not exist", epoch_num.as_u64()))?;
    if epoch_hash != target_epoch.block_info.hash {
        eyre::bail!("epoch hash mismatch with L1");
    }
    if timestamp.as_u64() != target_epoch.block_info.timestamp {
        eyre::bail!("epoch timestamp mismatch with L1");
    }
    if base_fee != target_epoch.block_info.base_fee {
        eyre::bail!("epoch base fee mismatch with L1");
    }
    if state_root != target_epoch.block_info.state_root {
        eyre::bail!("epoch state root mismatch with L1");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    mod decode_batches {
        use std::sync::{Arc, RwLock};

        use ethers::{
            types::{
                transaction::eip2718::TypedTransaction, Bytes, Signature, TransactionRequest, H256,
            },
            utils::rlp::{Encodable, RlpStream},
        };

        use crate::{
            common::RawTransaction,
            config::{ChainConfig, Config},
            derive::state::State,
            specular::{
                common::{
                    SetL1OracleValuesInput, SET_L1_ORACLE_VALUES_ABI, SET_L1_ORACLE_VALUES_SELECTOR,
                },
                config::SystemAccounts,
                stages::{
                    batcher_transactions::SpecularBatcherTransaction, batches::decode_batches,
                },
            },
        };

        struct SubBatch {
            first_l2_block_num: u64,
            tx_blocks: Vec<Vec<RawTransaction>>,
        }

        impl SubBatch {
            fn new(first_l2_block_num: u64, tx_blocks: Vec<Vec<RawTransaction>>) -> Self {
                Self {
                    first_l2_block_num,
                    tx_blocks,
                }
            }
        }

        impl Encodable for SubBatch {
            fn rlp_append(&self, s: &mut RlpStream) {
                s.begin_list(2);
                s.append(&self.first_l2_block_num);
                s.begin_list(self.tx_blocks.len());
                for tx_block in &self.tx_blocks {
                    s.begin_list(tx_block.len());
                    for tx in tx_block {
                        s.append(&tx.0);
                    }
                }
            }
        }

        struct BatcherTransactionData {
            version: u8,
            sub_batches: Vec<SubBatch>,
        }

        impl BatcherTransactionData {
            fn new(version: u8, sub_batches: Vec<SubBatch>) -> Self {
                Self {
                    version,
                    sub_batches,
                }
            }

            fn encode_sub_batches(&self) -> bytes::Bytes {
                let mut rlp = RlpStream::new();
                rlp.append_list(&self.sub_batches);
                rlp.out().freeze()
            }
        }

        #[test]
        fn decode() -> eyre::Result<()> {
            let config = Arc::new(Config {
                l1_rpc_url: Default::default(),
                l2_rpc_url: Default::default(),
                l2_engine_url: Default::default(),
                chain: ChainConfig::optimism(),
                jwt_secret: Default::default(),
                checkpoint_sync_url: Default::default(),
                rpc_port: Default::default(),
                devnet: false,
                local_sequencer: Default::default(),
            });
            let state = RwLock::new(State::new(
                Default::default(),
                Default::default(),
                config.clone(),
            ));

            let epoch_num: u64 = 1;
            let epoch_hash: H256 = Default::default();
            let timestamp: u64 = 2;
            let base_fee: u64 = 1;
            let state_root: H256 = Default::default();
            let first_l2_block_num = 1;
            let l1_inclusion_block = 1;

            let fake_signature = Signature {
                v: 0,
                r: Default::default(),
                s: Default::default(),
            };
            let oracle_tx_values: SetL1OracleValuesInput = (
                epoch_num.into(),
                timestamp.into(),
                base_fee.into(),
                epoch_hash,
                state_root,
                config.chain.system_config.l1_fee_overhead,
                config.chain.system_config.l1_fee_scalar,
            );
            let oracle_tx_data = SET_L1_ORACLE_VALUES_ABI
                .encode_with_selector(*SET_L1_ORACLE_VALUES_SELECTOR, oracle_tx_values)?;
            let oracle_tx: TypedTransaction = TransactionRequest::new()
                .to(SystemAccounts::default().l1_oracle)
                .data(oracle_tx_data)
                .into();
            let encoded_oracle_tx = oracle_tx.rlp_signed(&fake_signature).to_vec();

            let non_oracle_tx: TypedTransaction = TransactionRequest::new().into();
            let encoded_non_oracle_tx = non_oracle_tx.rlp_signed(&fake_signature).to_vec();

            let batch1 = SubBatch::new(
                first_l2_block_num,
                vec![
                    vec![RawTransaction(encoded_oracle_tx.clone())],
                    vec![RawTransaction(encoded_non_oracle_tx.clone())],
                ],
            );
            let batch2 = SubBatch::new(
                first_l2_block_num + 2,
                vec![vec![RawTransaction(encoded_non_oracle_tx.clone())]],
            );

            let version: u8 = 0;
            let batch = BatcherTransactionData::new(version, vec![batch1, batch2]);

            let batcher_tx = SpecularBatcherTransaction {
                l1_inclusion_block,
                version: batch.version,
                tx_batch: Bytes(batch.encode_sub_batches()),
            };
            let batches = decode_batches(&batcher_tx, &state, &config)?;

            assert_eq!(batches.len(), 3);

            assert_eq!(batches[0].epoch_num, epoch_num);
            assert_eq!(batches[0].epoch_hash, epoch_hash);
            assert_eq!(batches[0].timestamp, timestamp);
            assert_eq!(batches[0].l2_block_number, first_l2_block_num);
            assert_eq!(batches[0].transactions.len(), 1);
            assert_eq!(batches[0].transactions[0].0, encoded_oracle_tx);
            assert_eq!(batches[0].l1_inclusion_block, l1_inclusion_block);
            assert_eq!(batches[0].l1_oracle_values, Some(oracle_tx_values));

            assert_eq!(batches[1].epoch_num, epoch_num);
            assert_eq!(batches[1].epoch_hash, epoch_hash);
            assert_eq!(
                batches[1].timestamp,
                timestamp + config.as_ref().chain.blocktime
            );
            assert_eq!(batches[1].l2_block_number, first_l2_block_num + 1);
            assert_eq!(batches[1].transactions.len(), 1);
            assert_eq!(batches[1].transactions[0].0, encoded_non_oracle_tx);
            assert_eq!(batches[1].l1_inclusion_block, l1_inclusion_block);
            assert_eq!(batches[1].l1_oracle_values, None);

            assert_eq!(batches[2].epoch_num, epoch_num);
            assert_eq!(batches[2].epoch_hash, epoch_hash);
            assert_eq!(
                batches[2].timestamp,
                timestamp + 2 * config.as_ref().chain.blocktime
            );
            assert_eq!(batches[2].l2_block_number, first_l2_block_num + 2);
            assert_eq!(batches[2].transactions.len(), 1);
            assert_eq!(batches[2].transactions[0].0, encoded_non_oracle_tx);
            assert_eq!(batches[2].l1_inclusion_block, l1_inclusion_block);
            assert_eq!(batches[2].l1_oracle_values, None);

            Ok(())
        }
    }
}
