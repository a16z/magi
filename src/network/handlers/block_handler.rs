use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::SystemTime;

use ethers::types::{Address, Bytes, Signature, H256};
use ethers::utils::keccak256;
use eyre::Result;
use libp2p::gossipsub::{IdentTopic, Message, MessageAcceptance, TopicHash};
use ssz_rs::{prelude::*, List, Vector, U256};
use tokio::sync::watch;

use crate::{common::RawTransaction, engine::ExecutionPayload};

use super::Handler;

pub struct BlockHandlerV1 {
    chain_id: u64,
    block_sender: Sender<ExecutionPayload>,
    unsafe_signer_recv: watch::Receiver<Address>,
}

impl Handler for BlockHandlerV1 {
    fn handle(&self, msg: Message) -> MessageAcceptance {
        tracing::debug!("received block");

        match decode_block_msg::<ExecutionPayloadV1SSZ>(msg.data) {
            Ok((payload, signature, payload_hash)) => {
                if self.block_valid(&payload, signature, payload_hash) {
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

impl BlockHandlerV1 {
    pub fn new(
        chain_id: u64,
        unsafe_recv: watch::Receiver<Address>,
    ) -> (Self, Receiver<ExecutionPayload>) {
        let (sender, recv) = channel();

        let handler = Self {
            chain_id,
            block_sender: sender,
            unsafe_signer_recv: unsafe_recv,
        };

        (handler, recv)
    }

    fn block_valid(
        &self,
        payload: &ExecutionPayload,
        sig: Signature,
        payload_hash: PayloadHash,
    ) -> bool {
        let current_timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let is_future = payload.timestamp.as_u64() > current_timestamp + 5;
        let is_past = payload.timestamp.as_u64() < current_timestamp - 60;
        let time_valid = !(is_future || is_past);

        let msg = payload_hash.signature_message(self.chain_id);
        let block_signer = *self.unsafe_signer_recv.borrow();
        let sig_valid = sig.verify(msg, block_signer).is_ok();

        time_valid && sig_valid
    }
}

fn decode_block_msg<T>(data: Vec<u8>) -> Result<(ExecutionPayload, Signature, PayloadHash)>
where
    T: SimpleSerialize,
    ExecutionPayload: From<T>,
{
    let mut decoder = snap::raw::Decoder::new();
    let decompressed = decoder.decompress_vec(&data)?;
    let sig_data = &decompressed[..65];
    let block_data = &decompressed[65..];

    let signature = Signature::try_from(sig_data)?;

    let payload: T = deserialize(block_data)?;
    let payload: ExecutionPayload = ExecutionPayload::from(payload);

    let payload_hash = PayloadHash::from(block_data);

    Ok((payload, signature, payload_hash))
}

struct PayloadHash(H256);

impl From<&[u8]> for PayloadHash {
    fn from(value: &[u8]) -> Self {
        Self(keccak256(value).into())
    }
}

impl PayloadHash {
    fn signature_message(&self, chain_id: u64) -> H256 {
        let domain = H256::zero();
        let chain_id = H256::from_low_u64_be(chain_id);
        let payload_hash = self.0;

        let data: Vec<u8> = [
            domain.as_bytes(),
            chain_id.as_bytes(),
            payload_hash.as_bytes(),
        ]
        .concat();

        keccak256(data).into()
    }
}

type Bytes32 = Vector<u8, 32>;
type VecAddress = Vector<u8, 20>;
type Transaction = List<u8, 1073741824>;

#[derive(SimpleSerialize, Default)]
struct ExecutionPayloadV1SSZ {
    pub parent_hash: Bytes32,
    pub fee_recipient: VecAddress,
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

impl From<ExecutionPayloadV1SSZ> for ExecutionPayload {
    fn from(value: ExecutionPayloadV1SSZ) -> Self {
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
            withdrawals: Vec::new(),
        }
    }
}

#[derive(SimpleSerialize, Default)]
struct ExecutionPayloadV2SSZ {
    pub parent_hash: Bytes32,
    pub fee_recipient: VecAddress,
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
    pub withdrawals: List<Withdrawal, 16>,
}

#[derive(SimpleSerialize, Default)]
struct Withdrawal {
    index: u64,
    validator_index: u64,
    address: VecAddress,
    amount: u64,
}

impl From<ExecutionPayloadV2SSZ> for ExecutionPayload {
    fn from(value: ExecutionPayloadV2SSZ) -> Self {
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
            withdrawals: Vec::new(),
        }
    }
}

fn convert_hash(bytes: Bytes32) -> H256 {
    H256::from_slice(bytes.as_slice())
}

fn convert_address(address: VecAddress) -> Address {
    Address::from_slice(address.as_slice())
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
