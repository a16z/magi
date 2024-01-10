use core::fmt::Debug;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::io::Read;
use std::sync::{Arc, RwLock};

use ethers::types::H256;
use ethers::utils::rlp::{DecoderError, Rlp};

use eyre::Result;
use libflate::zlib::Decoder;

use crate::common::RawTransaction;
use crate::config::Config;
use crate::derive::state::State;
use crate::derive::PurgeableIterator;

use super::channels::Channel;
use super::span_batch::{BlockInput, SpanBatch};

pub struct Batches<I> {
    /// Mapping of timestamps to batches
    batches: BTreeMap<u64, Batch>,
    channel_iter: I,
    state: Arc<RwLock<State>>,
    config: Arc<Config>,
}

impl<I> Iterator for Batches<I>
where
    I: Iterator<Item = Channel>,
{
    type Item = Batch;

    fn next(&mut self) -> Option<Self::Item> {
        self.try_next().unwrap_or_else(|_| {
            tracing::debug!("Failed to decode batch");
            None
        })
    }
}

impl<I> PurgeableIterator for Batches<I>
where
    I: PurgeableIterator<Item = Channel>,
{
    fn purge(&mut self) {
        self.channel_iter.purge();
        self.batches.clear();
    }
}

impl<I> Batches<I> {
    pub fn new(channel_iter: I, state: Arc<RwLock<State>>, config: Arc<Config>) -> Self {
        Self {
            batches: BTreeMap::new(),
            channel_iter,
            state,
            config,
        }
    }
}

impl<I> Batches<I>
where
    I: Iterator<Item = Channel>,
{
    fn try_next(&mut self) -> Result<Option<Batch>> {
        let channel = self.channel_iter.next();
        if let Some(channel) = channel {
            let batches = decode_batches(&channel)?;
            batches.into_iter().for_each(|batch| {
                tracing::debug!(
                    "saw batch: t={}, ph={:?}, e={}",
                    batch.timestamp,
                    batch.parent_hash,
                    batch.epoch_num
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
                    BatchStatus::Future | BatchStatus::Undecided => {
                        break None;
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

                    Some(Batch {
                        epoch_num: epoch.number,
                        epoch_hash: epoch.hash,
                        parent_hash: safe_head.parent_hash,
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
            derived_batch
        };

        Ok(batch)
    }

    fn batch_status(&self, batch: &Batch) -> BatchStatus {
        match batch {
            Batch::Single(batch) => self.single_batch_status(batch),
            Batch::Span(_) => panic!("not implemented"),
        }
    }

    fn single_batch_status(&self, batch: &SingleBatch) -> BatchStatus {
        let state = self.state.read().unwrap();
        let epoch = state.safe_epoch;
        let next_epoch = state.epoch_by_number(epoch.number + 1);
        let head = state.safe_head;
        let next_timestamp = head.timestamp + self.config.chain.blocktime;

        // check timestamp range
        match batch.timestamp.cmp(&next_timestamp) {
            Ordering::Greater => return BatchStatus::Future,
            Ordering::Less => return BatchStatus::Drop,
            Ordering::Equal => (),
        }

        // check that block builds on existing chain
        if batch.parent_hash != head.hash {
            tracing::warn!("invalid parent hash");
            return BatchStatus::Drop;
        }

        // check the inclusion delay
        if batch.epoch_num + self.config.chain.seq_window_size < batch.l1_inclusion_block {
            tracing::warn!("inclusion window elapsed");
            return BatchStatus::Drop;
        }

        // check and set batch origin epoch
        let batch_origin = if batch.epoch_num == epoch.number {
            Some(epoch)
        } else if batch.epoch_num == epoch.number + 1 {
            next_epoch
        } else {
            tracing::warn!("invalid batch origin epoch number");
            return BatchStatus::Drop;
        };

        if let Some(batch_origin) = batch_origin {
            if batch.epoch_hash != batch_origin.hash {
                tracing::warn!("invalid epoch hash");
                return BatchStatus::Drop;
            }

            if batch.timestamp < batch_origin.timestamp {
                tracing::warn!("batch too old");
                return BatchStatus::Drop;
            }

            // handle sequencer drift
            if batch.timestamp > batch_origin.timestamp + self.config.chain.max_seq_drift {
                if batch.transactions.is_empty() {
                    if epoch.number == batch.epoch_num {
                        if let Some(next_epoch) = next_epoch {
                            if batch.timestamp >= next_epoch.timestamp {
                                tracing::warn!("sequencer drift too large");
                                return BatchStatus::Drop;
                            }
                        } else {
                            tracing::debug!("sequencer drift undecided");
                            return BatchStatus::Undecided;
                        }
                    }
                } else {
                    tracing::warn!("sequencer drift too large");
                    return BatchStatus::Drop;
                }
            }
        } else {
            tracing::debug!("batch origin not known");
            return BatchStatus::Undecided;
        }

        if batch.has_invalid_transactions() {
            tracing::warn!("invalid transaction");
            return BatchStatus::Drop;
        }

        BatchStatus::Accept
    }

    fn span_batch_status(&self, batch: &SpanBatch) -> BatchStatus {
        let state = self.state.read().unwrap();
        let epoch = state.safe_epoch;
        let next_epoch = state.epoch_by_number(epoch.number + 1);
        let head = state.safe_head;
        let next_timestamp = head.timestamp + self.config.chain.blocktime;

        let batch_timestamp = batch.rel_timestamp + self.config.chain.l2_genesis.timestamp;
        if batch_timestamp < self.config.chain.delta_time {
            return BatchStatus::Drop;
        }

        // check timestamp range

        match batch_timestamp.cmp(&next_timestamp) {
            Ordering::Greater => return BatchStatus::Future,
            Ordering::Less => return BatchStatus::Drop,
            Ordering::Equal => (),
        }

        let span_end_timestamp = batch_timestamp + batch.block_count * self.config.chain.blocktime;
        if span_end_timestamp < next_timestamp {
            return BatchStatus::Drop;
        }

        // check that block builds on existing chain

        if head.hash.as_bytes()[..20] != batch.parent_check {
            return BatchStatus::Drop;
        }

        // sequencer window checks

        let origin_changed_bit = batch.origin_bits[0];
        let end_epoch_num = batch.l1_origin_num;
        let start_epoch_num = batch.l1_origin_num
            - batch
                .origin_bits
                .iter()
                .map(|b| if *b { 1 } else { 0 })
                .sum::<u64>()
            + if origin_changed_bit { 1 } else { 0 };

        if start_epoch_num + self.config.chain.seq_window_size < batch.l1_inclusion_block {
            return BatchStatus::Drop;
        }

        // is this right?
        if start_epoch_num > epoch.number + 1 {
            return BatchStatus::Drop;
        }

        if let Some(l1_origin) = state.epoch_by_number(end_epoch_num) {
            if batch.l1_origin_check != l1_origin.hash.as_bytes()[..20] {
                return BatchStatus::Drop;
            }
        } else {
            return BatchStatus::Drop;
        }

        if start_epoch_num < epoch.number {
            return BatchStatus::Drop;
        }

        // check sequencer drift

        let block_inputs = batch.block_inputs(&self.config);
        for (i, input) in block_inputs.iter().enumerate() {
            let input_epoch = state.epoch_by_number(input.epoch_num).unwrap();
            let next_epoch = state.epoch_by_number(input.epoch_num + 1);

            if input.timestamp < input_epoch.timestamp {
                return BatchStatus::Drop;
            }

            if input.timestamp > input_epoch.timestamp + self.config.chain.max_seq_drift {
                if input.transactions.len() == 0 {
                    if !batch.origin_bits[i] {
                        if let Some(next_epoch) = next_epoch {
                            if input.timestamp >= next_epoch.timestamp {
                                return BatchStatus::Drop;
                            }
                        } else {
                            return BatchStatus::Undecided;
                        }
                    }
                } else {
                    return BatchStatus::Drop;
                }
            }
        }

        // overlapped block checks

        for input in block_inputs {
            if input.timestamp >= next_timestamp {
                // TODO: compare with existing L2 blocks
            }
        }

        BatchStatus::Accept
    }
}

fn decode_batches(channel: &Channel) -> Result<Vec<Batch>> {
    let mut channel_data = Vec::new();
    let d = Decoder::new(channel.data.as_slice())?;
    for b in d.bytes() {
        if let Ok(b) = b {
            channel_data.push(b);
        } else {
            break;
        }
    }

    let mut batches = Vec::new();
    let mut offset = 0;

    while offset < channel_data.len() {
        let batch_rlp = Rlp::new(&channel_data[offset..]);
        let batch_info = batch_rlp.payload_info()?;

        let batch_data: Vec<u8> = batch_rlp.as_val()?;

        let version = batch_data[0];
        let batch_content = &batch_data[1..];

        match version {
            0 => {
                let rlp = Rlp::new(batch_content);
                let size = rlp.payload_info()?.total();

                let batch = SingleBatch::decode(&rlp, channel.l1_inclusion_block)?;
                batches.push(Batch::Single(batch));

                offset += size + batch_info.header_len + 1;
            }
            1 => {
                let batch = SpanBatch::decode(batch_content, channel.l1_inclusion_block)?;
                batches.push(Batch::Span(batch));
                break;
            }
            _ => eyre::bail!("invalid batch version"),
        };
    }

    Ok(batches)
}

#[derive(Debug, Clone)]
pub enum Batch {
    Single(SingleBatch),
    Span(SpanBatch),
}

#[derive(Debug, Clone)]
pub struct SingleBatch {
    pub parent_hash: H256,
    pub epoch_num: u64,
    pub epoch_hash: H256,
    pub timestamp: u64,
    pub transactions: Vec<RawTransaction>,
    pub l1_inclusion_block: u64,
}

#[derive(Debug, Clone, PartialEq)]
enum BatchStatus {
    Drop,
    Accept,
    Undecided,
    Future,
}

impl SingleBatch {
    fn decode(rlp: &Rlp, l1_inclusion_block: u64) -> Result<Self, DecoderError> {
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

    fn has_invalid_transactions(&self) -> bool {
        self.transactions
            .iter()
            .any(|tx| tx.0.is_empty() || tx.0[0] == 0x7E)
    }
}
