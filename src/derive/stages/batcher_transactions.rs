use std::sync::{Arc, Mutex};

use eyre::Result;

pub struct BatcherTransactions {
    txs: Vec<BatcherTransaction>,
}

impl Iterator for BatcherTransactions {
    type Item = BatcherTransaction;

    fn next(&mut self) -> Option<Self::Item> {
        if !self.txs.is_empty() {
            Some(self.txs.remove(0))
        } else {
            None
        }
    }
}

impl BatcherTransactions {
    pub fn new() -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self { txs: Vec::new() }))
    }

    pub fn push_data(&mut self, txs: Vec<Vec<u8>>, l1_origin: u64) {
        for data in txs {
            let res = BatcherTransaction::new(&data, l1_origin).map(|tx| {
                self.txs.push(tx);
            });

            if res.is_err() {
                tracing::debug!("Failed to decode batcher transaction");
            }
        }
    }

    pub fn purge(&mut self) {
        self.txs.clear();
    }
}

#[derive(Debug, Clone)]
pub struct BatcherTransaction {
    pub version: u8,
    pub frames: Vec<Frame>,
}

impl BatcherTransaction {
    fn new(data: &[u8], l1_origin: u64) -> Result<Self> {
        let version = data[0];
        let frame_data = data.get(1..).ok_or(eyre::eyre!("No frame data"))?;

        let mut offset = 0;
        let mut frames = Vec::new();
        while offset < frame_data.len() {
            let (frame, next_offset) = Frame::from_data(frame_data, offset, l1_origin)?;
            frames.push(frame);
            offset = next_offset;
        }

        Ok(Self { version, frames })
    }
}

#[derive(Debug, Default, Clone)]
pub struct Frame {
    pub channel_id: u128,
    pub frame_number: u16,
    pub frame_data_len: u32,
    pub frame_data: Vec<u8>,
    pub is_last: bool,
    pub l1_origin: u64,
}

impl Frame {
    fn from_data(data: &[u8], offset: usize, l1_origin: u64) -> Result<(Self, usize)> {
        let data = &data[offset..];

        let channel_id = u128::from_be_bytes(data[0..16].try_into()?);
        let frame_number = u16::from_be_bytes(data[16..18].try_into()?);
        let frame_data_len = u32::from_be_bytes(data[18..22].try_into()?);

        let frame_data_end = 22 + frame_data_len as usize;
        let frame_data = data[22..frame_data_end].to_vec();

        let is_last = data[frame_data_end] != 0;

        let frame = Self {
            channel_id,
            frame_number,
            frame_data_len,
            frame_data,
            is_last,
            l1_origin,
        };

        Ok((frame, offset + data.len()))
    }
}
