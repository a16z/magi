use core::fmt::Debug;
use std::io::Read;

use ethers::{
    types::H256,
    utils::rlp::{decode, Decodable, Rlp},
};
use eyre::Result;
use libflate::zlib::Decoder;

use crate::channel_bank::Channel;

pub fn decode_batches(channel: &Channel) -> Result<Vec<Batch>> {
    let mut channel_data = Vec::new();
    let mut d = Decoder::new(channel.data.as_slice())?;
    d.read_to_end(&mut channel_data)?;

    let channel_data: Vec<u8> = decode(&channel_data)?;
    let batch_content: Vec<u8> = channel_data[1..].to_vec();

    // TODO: handle multiple batches in one channel
    let batch: Batch = decode(&batch_content)?;

    Ok(vec![batch])
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
    fn decode(rlp: &Rlp) -> Result<Self, ethers::utils::rlp::DecoderError> {
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

pub struct RawTransaction(Vec<u8>);

impl Decodable for RawTransaction {
    fn decode(rlp: &Rlp) -> Result<Self, ethers::utils::rlp::DecoderError> {
        let tx_bytes: Vec<u8> = rlp.as_val()?;
        Ok(Self(tx_bytes))
    }
}

impl Debug for RawTransaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x{}", hex::encode(&self.0))
    }
}
