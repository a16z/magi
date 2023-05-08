use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::SystemTime;

use ethers::types::{Bytes, H256};
use eyre::Result;
use libp2p::gossipsub::{IdentTopic, Message, MessageAcceptance, TopicHash};
use ssz_rs::{prelude::*, List, Vector, U256};

use crate::{common::RawTransaction, engine::ExecutionPayload};

use super::Handler;

pub struct BlockHandler {
    chain_id: u64,
    block_sender: Sender<ExecutionPayload>,
}

impl Handler for BlockHandler {
    fn handle(&self, msg: Message) -> MessageAcceptance {
        tracing::debug!("received block");

        match decode_block_msg(msg.data) {
            Ok(payload) => {
                if block_valid(&payload) {
                    _ = self.block_sender.send(payload);
                    MessageAcceptance::Accept
                } else {
                    tracing::warn!("invalid unsafe block");
                    MessageAcceptance::Reject
                }
            }
            Err(err) => {
                tracing::warn!("unsafe block decode failed: {}", err);
                MessageAcceptance::Reject
            }
        }
    }

    fn topic(&self) -> TopicHash {
        IdentTopic::new(format!("/optimism/{}/0/blocks", self.chain_id)).into()
    }
}

impl BlockHandler {
    pub fn new(chain_id: u64) -> (Self, Receiver<ExecutionPayload>) {
        let (sender, recv) = channel();

        let handler = Self {
            chain_id,
            block_sender: sender,
        };

        (handler, recv)
    }
}

fn block_valid(payload: &ExecutionPayload) -> bool {
    let current_timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let is_future = payload.timestamp.as_u64() > current_timestamp + 5;
    let is_past = payload.timestamp.as_u64() < current_timestamp - 60;

    !(is_future || is_past)
}

fn decode_block_msg(data: Vec<u8>) -> Result<ExecutionPayload> {
    let mut decoder = snap::raw::Decoder::new();
    let decompressed = decoder.decompress_vec(&data)?;
    let block_data = &decompressed[65..];
    let payload: ExecutionPayloadSSZ = deserialize(block_data)?;
    Ok(ExecutionPayload::from(payload))
}

type Bytes32 = Vector<u8, 32>;
type Address = Vector<u8, 20>;
type Transaction = List<u8, 1073741824>;

#[derive(SimpleSerialize, Default)]
struct ExecutionPayloadSSZ {
    pub parent_hash: Bytes32,
    pub fee_recipient: Address,
    pub state_root: Bytes32,
    pub receipts_root: Bytes32,
    pub logs_bloom: Vector<u8, 256>,
    pub prev_randao: Bytes32,
    pub block_number: u64,
    pub gas_limit: u64,
    pub gas_used: u64,
    pub timestamp: u64,
    pub extra_data: List<u8, 32>,
    pub base_fee_per_gas: U256,
    pub block_hash: Bytes32,
    pub transactions: List<Transaction, 1048576>,
}

impl From<ExecutionPayloadSSZ> for ExecutionPayload {
    fn from(value: ExecutionPayloadSSZ) -> Self {
        Self {
            parent_hash: convert_hash(value.parent_hash),
            fee_recipient: convert_address(value.fee_recipient),
            state_root: convert_hash(value.state_root),
            receipts_root: convert_hash(value.receipts_root),
            logs_bloom: convert_byte_vector(value.logs_bloom),
            prev_randao: convert_hash(value.prev_randao),
            block_number: value.block_number.into(),
            gas_limit: value.gas_limit.into(),
            gas_used: value.gas_used.into(),
            timestamp: value.timestamp.into(),
            extra_data: convert_byte_list(value.extra_data),
            base_fee_per_gas: convert_uint(value.base_fee_per_gas),
            block_hash: convert_hash(value.block_hash),
            transactions: convert_tx_list(value.transactions),
        }
    }
}

fn convert_hash(bytes: Bytes32) -> H256 {
    H256::from_slice(bytes.as_slice())
}

fn convert_address(address: Address) -> ethers::types::Address {
    ethers::types::Address::from_slice(address.as_slice())
}

fn convert_byte_vector<const N: usize>(vector: Vector<u8, N>) -> Bytes {
    Bytes::from(vector.to_vec())
}

fn convert_byte_list<const N: usize>(list: List<u8, N>) -> Bytes {
    Bytes::from(list.to_vec())
}

fn convert_uint(value: U256) -> ethers::types::U64 {
    let bytes = value.to_bytes_le();
    ethers::types::U256::from_little_endian(&bytes)
        .as_u64()
        .into()
}

fn convert_tx_list(value: List<Transaction, 1048576>) -> Vec<RawTransaction> {
    value.iter().map(|tx| RawTransaction(tx.to_vec())).collect()
}
