use core::fmt::Debug;
use std::sync::Arc;
use std::{cell::RefCell, io::Read, rc::Rc};

use ethers_core::types::H256;
use ethers_core::utils::rlp::{Decodable, DecoderError, Rlp};

use eyre::Result;
use libflate::zlib::Decoder;

use crate::common::{Epoch, RawTransaction, BlockInfo};
use crate::config::Config;

use super::channels::{Channel, Channels};

pub struct Batches {
    batches: Vec<Batch>,
    prev_stage: Rc<RefCell<Channels>>,
    safe_epoch: Rc<RefCell<Epoch>>,
    safe_head: Rc<RefCell<BlockInfo>>,
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
        prev_stage: Rc<RefCell<Channels>>,
        safe_epoch: Rc<RefCell<Epoch>>,
        safe_head: Rc<RefCell<BlockInfo>>,
        config: Arc<Config>,
    ) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            batches: Vec::new(),
            prev_stage,
            safe_epoch,
            safe_head,
            config,
        }))
    }

    fn try_next(&mut self) -> Result<Option<Batch>> {
        let channel = self.prev_stage.borrow_mut().next();
        if let Some(channel) = channel {
            let mut batches = decode_batches(&channel)?;
            self.batches.append(&mut batches);
        }

        self.batches.sort_by_key(|b| b.timestamp);

        self.batches = self
            .batches
            .clone()
            .into_iter()
            .map(|b| self.set_batch_status(b))
            .filter(|b| !b.is_dropped())
            .collect();

        let pos = self
            .batches
            .iter()
            .position(|b| b.status == BatchStatus::Accept);

        Ok(if let Some(pos) = pos {
            Some(self.batches.remove(pos))
        } else {
            None
        })
    }

    fn set_batch_status(&self, mut batch: Batch) -> Batch {
        let epoch = self.safe_epoch.borrow();
        let head = self.safe_head.borrow();
        let next_timestamp = head.timestamp + 2;

        if batch.timestamp > next_timestamp {
            batch.status = BatchStatus::Future;
            return batch;
        } else if batch.timestamp < next_timestamp {
            batch.status = BatchStatus::Drop;
            return batch;
        }

        if batch.parent_hash != head.hash {
            batch.status = BatchStatus::Drop;
            return batch;
        }

        // TODO: inclusion window check

        let _batch_origin = if batch.epoch_num == epoch.number {
            Some(epoch)
        } else if batch.epoch_num == epoch.number + 1 {
            None
        } else {
            batch.status = BatchStatus::Drop;
            return batch;
        };

        // TODO: batch origin checks

        if batch.has_invalid_transactions() {
            batch.status = BatchStatus::Drop;
            return batch;
        }

        batch.status = BatchStatus::Accept;
        batch
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
    status: BatchStatus,
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
            status: BatchStatus::Accept,
        })
    }
}

impl Batch {
    fn has_invalid_transactions(&self) -> bool {
        self.transactions
            .iter()
            .filter(|b| b.0.len() == 0 || b.0[0] == 0x7E)
            .next()
            .is_some()
    }

    fn is_dropped(&self) -> bool {
        let dropped = self.status == BatchStatus::Drop;
        if dropped {
            tracing::info!("dropped invalid batch");
        }

        dropped
    }
}
