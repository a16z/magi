use core::fmt::Debug;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::io::Read;
use std::sync::{Arc, RwLock};

use ethers::utils::rlp::Rlp;
use eyre::Result;
use libflate::zlib::Decoder;

use crate::config::Config;
use crate::derive::state::State;
use crate::derive::PurgeableIterator;

use super::block_input::BlockInput;
use super::channels::Channel;
use super::single_batch::SingleBatch;
use super::span_batch::SpanBatch;

pub struct Batches<I> {
    /// Mapping of timestamps to batches
    batches: BTreeMap<u64, Batch>,
    /// Pending block inputs to be outputed
    pending_inputs: Vec<BlockInput<u64>>,
    channel_iter: I,
    state: Arc<RwLock<State>>,
    config: Arc<Config>,
}

impl<I> Iterator for Batches<I>
where
    I: Iterator<Item = Channel>,
{
    type Item = BlockInput<u64>;

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
        self.pending_inputs.clear();
    }
}

impl<I> Batches<I> {
    pub fn new(channel_iter: I, state: Arc<RwLock<State>>, config: Arc<Config>) -> Self {
        Self {
            batches: BTreeMap::new(),
            pending_inputs: Vec::new(),
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
    fn try_next(&mut self) -> Result<Option<BlockInput<u64>>> {
        if !self.pending_inputs.is_empty() {
            return Ok(Some(self.pending_inputs.remove(0)));
        }

        let channel = self.channel_iter.next();
        if let Some(channel) = channel {
            let batches = decode_batches(&channel, self.config.chain.l2_chain_id)?;
            batches.into_iter().for_each(|batch| {
                let timestamp = batch.timestamp(&self.config);
                tracing::debug!("saw batch: t={}", timestamp);
                self.batches.insert(timestamp, batch);
            });
        }

        let derived_batch = loop {
            if let Some((_, batch)) = self.batches.first_key_value() {
                let timestamp = batch.timestamp(&self.config);
                match self.batch_status(batch) {
                    BatchStatus::Accept => {
                        let batch = batch.clone();
                        self.batches.remove(&timestamp);
                        break Some(batch);
                    }
                    BatchStatus::Drop => {
                        tracing::warn!("dropping invalid batch");
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

        Ok(if let Some(derived_batch) = derived_batch {
            let mut inputs = self.filter_inputs(derived_batch.as_inputs(&self.config));
            if !inputs.is_empty() {
                let first = inputs.remove(0);
                self.pending_inputs.append(&mut inputs);
                Some(first)
            } else {
                None
            }
        } else {
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

                    // TODO: REMOVE TESTING ONLY
                    if next_timestamp >= self.config.chain.delta_time {
                        panic!("attempted to insert empty batch after delta")
                    }

                    Some(BlockInput {
                        epoch: epoch.number,
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
        })
    }

    fn filter_inputs(&self, inputs: Vec<BlockInput<u64>>) -> Vec<BlockInput<u64>> {
        inputs
            .into_iter()
            .filter(|input| input.timestamp > self.state.read().unwrap().safe_head.timestamp)
            .collect()
    }

    fn batch_status(&self, batch: &Batch) -> BatchStatus {
        match batch {
            Batch::Single(batch) => self.single_batch_status(batch),
            Batch::Span(batch) => self.span_batch_status(batch),
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

        let span_start_timestamp = batch.rel_timestamp + self.config.chain.l2_genesis.timestamp;
        let span_end_timestamp =
            span_start_timestamp + batch.block_count * self.config.chain.blocktime;

        let prev_timestamp = span_start_timestamp - self.config.chain.blocktime;
        let (prev_l2_block, prev_l2_epoch) =
            if let Some(block) = state.l2_info_by_timestamp(prev_timestamp) {
                block
            } else {
                tracing::warn!("prev l2 block not found");
                return BatchStatus::Drop;
            };

        let start_epoch_num = batch.start_epoch_num();
        let end_epoch_num = batch.l1_origin_num;

        // check for delta activation

        let batch_origin = if start_epoch_num == epoch.number + 1 {
            next_epoch
        } else {
            Some(epoch)
        };

        if let Some(batch_origin) = batch_origin {
            if batch_origin.timestamp < self.config.chain.delta_time {
                tracing::warn!("span batch seen before delta start");
                return BatchStatus::Drop;
            }
        } else {
            return BatchStatus::Undecided;
        }

        // check timestamp range

        if span_start_timestamp > next_timestamp {
            return BatchStatus::Future;
        }

        if span_end_timestamp < next_timestamp {
            tracing::warn!("span batch ends before next block");
            return BatchStatus::Drop;
        }

        // check that block builds on existing chain

        if prev_l2_block.hash.as_bytes()[..20] != batch.parent_check {
            tracing::warn!("batch parent check failed");
            return BatchStatus::Drop;
        }

        // sequencer window checks

        if start_epoch_num + self.config.chain.seq_window_size < batch.l1_inclusion_block {
            tracing::warn!("sequence window check failed");
            return BatchStatus::Drop;
        }

        if start_epoch_num > prev_l2_block.number + 1 {
            tracing::warn!("invalid start epoch number");
            return BatchStatus::Drop;
        }

        if let Some(l1_origin) = state.epoch_by_number(end_epoch_num) {
            if batch.l1_origin_check != l1_origin.hash.as_bytes()[..20] {
                tracing::warn!("origin check failed");
                return BatchStatus::Drop;
            }
        } else {
            tracing::warn!("origin not found");
            return BatchStatus::Drop;
        }

        if start_epoch_num < prev_l2_epoch.number {
            tracing::warn!("invalid start epoch number");
            return BatchStatus::Drop;
        }

        // check sequencer drift

        let block_inputs = batch.block_inputs(&self.config);
        for (i, input) in block_inputs.iter().enumerate() {
            let input_epoch = state.epoch_by_number(input.epoch).unwrap();
            let next_epoch = state.epoch_by_number(input.epoch + 1);

            if input.timestamp < input_epoch.timestamp {
                return BatchStatus::Drop;
            }

            if input.timestamp > input_epoch.timestamp + self.config.chain.max_seq_drift {
                if input.transactions.is_empty() {
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
            if input.timestamp < next_timestamp {
                if let Some(_) = state.l2_info_by_timestamp(input.timestamp) {
                    // check overlapped blocks
                } else {
                    tracing::warn!("overlapped l2 block not found");
                    return BatchStatus::Drop;
                }
            }
        }

        BatchStatus::Accept
    }
}

fn decode_batches(channel: &Channel, chain_id: u64) -> Result<Vec<Batch>> {
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
                let batch = SpanBatch::decode(batch_content, channel.l1_inclusion_block, chain_id)?;
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

impl Batch {
    pub fn timestamp(&self, config: &Config) -> u64 {
        match self {
            Batch::Single(batch) => batch.timestamp,
            Batch::Span(batch) => batch.rel_timestamp + config.chain.l2_genesis.timestamp,
        }
    }

    pub fn as_inputs(&self, config: &Config) -> Vec<BlockInput<u64>> {
        match self {
            Batch::Single(batch) => vec![batch.block_input()],
            Batch::Span(batch) => batch.block_inputs(config),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum BatchStatus {
    Drop,
    Accept,
    Undecided,
    Future,
}
