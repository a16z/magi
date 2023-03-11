use core::fmt::Debug;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::io::Read;
use std::sync::{Arc, Mutex, RwLock};

use ethers_core::types::H256;
use ethers_core::utils::rlp::{Decodable, DecoderError, Rlp};

use eyre::Result;
use libflate::zlib::Decoder;

use crate::common::RawTransaction;
use crate::config::Config;
use crate::derive::state::State;

use super::channels::{Channel, Channels};

pub struct Batches {
    /// Mapping of timestamps to batches
    batches: BTreeMap<u64, Batch>,
    prev_stage: Arc<Mutex<Channels>>,
    state: Arc<RwLock<State>>,
    config: Arc<Config>,
}

impl Iterator for Batches {
    type Item = Batch;

    fn next(&mut self) -> Option<Self::Item> {
        self.try_next().unwrap_or_else(|_| {
            tracing::debug!("Failed to decode batch");
            None
        })
    }
}

impl Batches {
    pub fn new(
        prev_stage: Arc<Mutex<Channels>>,
        state: Arc<RwLock<State>>,
        config: Arc<Config>,
    ) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            batches: BTreeMap::new(),
            prev_stage,
            state,
            config,
        }))
    }

    fn try_next(&mut self) -> Result<Option<Batch>> {
        let channel = self.prev_stage.lock().unwrap().next();
        if let Some(channel) = channel {
            let batches = decode_batches(&channel)?;
            batches.into_iter().for_each(|batch| {
                self.batches.insert(batch.timestamp, batch);
            });
        }

        let batch = loop {
            if let Some((_, batch)) = self.batches.first_key_value() {
                match self.batch_status(batch) {
                    BatchStatus::Accept => {
                        let batch = batch.clone();
                        self.batches.remove(&batch.timestamp);
                        break Some(batch);
                    }
                    BatchStatus::Drop => {
                        tracing::info!("dropping invalid batch");
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

        Ok(batch)
    }

    fn batch_status(&self, batch: &Batch) -> BatchStatus {
        let state = self.state.read().unwrap();
        let epoch = state.safe_epoch;
        let next_epoch = state.epoch_by_number(epoch.number + 1);
        let head = state.safe_head;
        let next_timestamp = head.timestamp + 2;

        // check timestamp range
        match batch.timestamp.cmp(&next_timestamp) {
            Ordering::Greater => return BatchStatus::Future,
            Ordering::Less => return BatchStatus::Drop,
            Ordering::Equal => (),
        }

        // check that block builds on existing chain
        if batch.parent_hash != head.hash {
            return BatchStatus::Drop;
        }

        // TODO: inclusion window check

        // check and set batch origin epoch
        let batch_origin = if batch.epoch_num == epoch.number {
            Some(epoch)
        } else if batch.epoch_num == epoch.number + 1 {
            next_epoch
        } else {
            return BatchStatus::Drop;
        };

        if let Some(batch_origin) = batch_origin {
            if batch.epoch_hash != batch_origin.hash {
                return BatchStatus::Drop;
            }

            if batch.timestamp < batch_origin.timestamp {
                return BatchStatus::Drop;
            }

            // handle sequencer drift
            if batch.timestamp > batch_origin.timestamp + self.config.chain.max_seq_drift {
                if batch.transactions.is_empty() {
                    if epoch.number == batch.epoch_num {
                        if let Some(next_epoch) = next_epoch {
                            if batch.timestamp >= next_epoch.timestamp {
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
        } else {
            return BatchStatus::Undecided;
        }

        if batch.has_invalid_transactions() {
            return BatchStatus::Drop;
        }

        BatchStatus::Accept
    }
}

fn decode_batches(channel: &Channel) -> Result<Vec<Batch>> {
    let mut channel_data = Vec::new();
    let mut d = Decoder::new(channel.data.as_slice())?;
    d.read_to_end(&mut channel_data)?;

    let mut batches = Vec::new();
    let mut offset = 0;

    while offset < channel_data.len() {
        let batch_rlp = Rlp::new(&channel_data[offset..]);
        let batch_info = batch_rlp.payload_info()?;

        let batch_data: Vec<u8> = batch_rlp.as_val()?;

        let batch_content = &batch_data[1..];
        let rlp = Rlp::new(batch_content);
        let size = rlp.payload_info()?.total();

        let batch: Batch = rlp.as_val()?;
        batches.push(batch);

        offset += size + batch_info.header_len + 1;
    }

    Ok(batches)
}

#[derive(Debug, Clone)]
pub struct Batch {
    pub parent_hash: H256,
    pub epoch_num: u64,
    pub epoch_hash: H256,
    pub timestamp: u64,
    pub transactions: Vec<RawTransaction>,
}

#[derive(Debug, Clone, PartialEq)]
enum BatchStatus {
    Drop,
    Accept,
    Undecided,
    Future,
}

impl Decodable for Batch {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        let parent_hash = rlp.val_at(0)?;
        let epoch_num = rlp.val_at(1)?;
        let epoch_hash = rlp.val_at(2)?;
        let timestamp = rlp.val_at(3)?;
        let transactions = rlp.list_at(4)?;

        Ok(Batch {
            parent_hash,
            epoch_num,
            epoch_hash,
            timestamp,
            transactions,
        })
    }
}

impl Batch {
    fn has_invalid_transactions(&self) -> bool {
        self.transactions
            .iter()
            .any(|tx| tx.0.is_empty() || tx.0[0] == 0x7E)
    }
}
