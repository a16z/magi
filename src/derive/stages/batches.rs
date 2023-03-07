use core::fmt::Debug;
use std::sync::{Arc, Mutex};
use std::io::Read;

use ethers_core::types::H256;
use ethers_core::utils::rlp::{Decodable, DecoderError, Rlp};

use eyre::Result;
use libflate::zlib::Decoder;

use crate::common::RawTransaction;

use super::channels::{Channel, Channels};

pub struct Batches {
    batches: Vec<Batch>,
    prev_stage: Arc<Mutex<Channels>>,
    start_epoch: u64,
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
    pub fn new(prev_stage: Arc<Mutex<Channels>>, start_epoch: u64) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            batches: Vec::new(),
            prev_stage,
            start_epoch,
        }))
    }

    fn try_next(&mut self) -> Result<Option<Batch>> {
        let channel = self.prev_stage.lock().ok().and_then(|mut c| c.next());
        if let Some(channel) = channel {
            let mut batches = decode_batches(&channel)?
                .into_iter()
                .filter(|b| b.epoch_num >= self.start_epoch)
                .collect();

            self.batches.append(&mut batches);
        }

        Ok(if !self.batches.is_empty() {
            Some(self.batches.remove(0))
        } else {
            None
        })
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

#[derive(Debug)]
pub struct Batch {
    pub parent_hash: H256,
    pub epoch_num: u64,
    pub epoch_hash: H256,
    pub timestamp: u64,
    pub transactions: Vec<RawTransaction>,
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
