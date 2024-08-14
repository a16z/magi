use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::SystemTime;

use alloy_primitives::{Address, Bytes, Signature, B256, keccak256};
use eyre::Result;
use libp2p::gossipsub::{IdentTopic, Message, MessageAcceptance, TopicHash};
use ssz_rs::{prelude::*, List, Vector, U256};
use tokio::sync::watch;

use crate::{common::RawTransaction, engine::ExecutionPayload};

use super::Handler;

/// Responsible for managing blocks received via p2p gossip
pub struct BlockHandler {
    /// Chain ID of the L2 blockchain. Used to filter out gossip messages intended for other blockchains.
    chain_id: u64,
    /// A channel sender to forward new blocks to other modules
    block_sender: Sender<ExecutionPayload>,
    /// A [watch::Receiver] to monitor changes to the unsafe block signer.
    unsafe_signer_recv: watch::Receiver<Address>,
    /// The libp2p topic for pre Canyon/Shangai blocks: `/optimism/{chain_id}/0/blocks`
    blocks_v1_topic: IdentTopic,
    /// The libp2p topic for Canyon/Delta blocks: `/optimism/{chain_id}/1/blocks`
    blocks_v2_topic: IdentTopic,
    blocks_v3_topic: IdentTopic,
}

struct ExecutionPayloadEnvelope {
    payload: ExecutionPayload,
    signature: Signature,
    hash: PayloadHash,
    #[allow(unused)]
    parent_beacon_block_root: Option<B256>,
}

impl Handler for BlockHandler {
    /// Checks validity of a block received via p2p gossip, and sends to the block update channel if valid.
    fn handle(&self, msg: Message) -> MessageAcceptance {
        tracing::debug!("received block");

        let decoded = if msg.topic == self.blocks_v1_topic.hash() {
            decode_pre_ecotone_block_msg::<ExecutionPayloadV1SSZ>(msg.data)
        } else if msg.topic == self.blocks_v2_topic.hash() {
            decode_pre_ecotone_block_msg::<ExecutionPayloadV2SSZ>(msg.data)
        } else if msg.topic == self.blocks_v3_topic.hash() {
            decode_post_ecotone_block_msg(msg.data)
        } else {
            return MessageAcceptance::Reject;
        };

        match decoded {
            Ok(envelope) => {
                if self.block_valid(&envelope) {
                    _ = self.block_sender.send(envelope.payload);
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

    /// The gossip topics accepted for new blocks
    fn topics(&self) -> Vec<TopicHash> {
        vec![self.blocks_v1_topic.hash(), self.blocks_v2_topic.hash()]
    }
}

impl BlockHandler {
    /// Creates a new [BlockHandler] and opens a channel
    pub fn new(
        chain_id: u64,
        unsafe_recv: watch::Receiver<Address>,
    ) -> (Self, Receiver<ExecutionPayload>) {
        let (sender, recv) = channel();

        let handler = Self {
            chain_id,
            block_sender: sender,
            unsafe_signer_recv: unsafe_recv,
            blocks_v1_topic: IdentTopic::new(format!("/optimism/{}/0/blocks", chain_id)),
            blocks_v2_topic: IdentTopic::new(format!("/optimism/{}/1/blocks", chain_id)),
            blocks_v3_topic: IdentTopic::new(format!("/optimism/{}/2/blocks", chain_id)),
        };

        (handler, recv)
    }

    /// Determines if a block is valid.
    ///
    /// True if the block is less than 1 minute old, and correctly signed by the unsafe block signer.
    fn block_valid(&self, envelope: &ExecutionPayloadEnvelope) -> bool {
        let current_timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let is_future = envelope.payload.timestamp.as_u64() > current_timestamp + 5;
        let is_past = envelope.payload.timestamp.as_u64() < current_timestamp - 60;
        let time_valid = !(is_future || is_past);

        let msg = envelope.hash.signature_message(self.chain_id);
        let block_signer = *self.unsafe_signer_recv.borrow();
        let sig_valid = envelope.signature.verify(msg, block_signer).is_ok();

        time_valid && sig_valid
    }
}

/// Decodes a sequence of bytes to an [ExecutionPayloadEnvelope]
fn decode_pre_ecotone_block_msg<T>(data: Vec<u8>) -> Result<ExecutionPayloadEnvelope>
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

    let hash = PayloadHash::from(block_data);

    Ok(ExecutionPayloadEnvelope {
        parent_beacon_block_root: None,
        signature,
        payload,
        hash,
    })
}

/// Decodes a sequence of bytes to an [ExecutionPayloadEnvelope]. The Ecotone V3
/// block topic encoding includes the parent beacon block root as described in the [specs].
///
/// [specs]: https://specs.optimism.io/protocol/rollup-node-p2p.html#block-encoding
fn decode_post_ecotone_block_msg(data: Vec<u8>) -> Result<ExecutionPayloadEnvelope> {
    let mut decoder = snap::raw::Decoder::new();
    let decompressed = decoder.decompress_vec(&data)?;
    let sig_data = &decompressed[..65];
    let parent_beacon_block_root = &decompressed[65..97];
    let block_data = &decompressed[97..];

    let signature = Signature::try_from(sig_data)?;

    let parent_beacon_block_root = Some(B256::from_slice(parent_beacon_block_root));

    let payload: ExecutionPayloadV3SSZ = deserialize(block_data)?;
    let payload = ExecutionPayload::from(payload);

    let hash = PayloadHash::from(block_data);

    Ok(ExecutionPayloadEnvelope {
        parent_beacon_block_root,
        signature,
        payload,
        hash,
    })
}

/// Represents the Keccak256 hash of the block
struct PayloadHash(B256);

impl From<&[u8]> for PayloadHash {
    /// Returns the Keccak256 hash of a sequence of bytes
    fn from(value: &[u8]) -> Self {
        Self(keccak256(value).into())
    }
}

impl PayloadHash {
    /// The expected message that should be signed by the unsafe block signer.
    fn signature_message(&self, chain_id: u64) -> B256 {
        let domain = B256::ZERO;
        let chain_id = B256::from(alloy_primitives::U256::from(chain_id));
        let payload_hash = self.0;

        let data: Vec<u8> = [
            domain.as_slice(),
            chain_id.as_slice(),
            payload_hash.as_slice(),
        ]
        .concat();

        keccak256(data).into()
    }
}

/// A type alias for a vector of 32 bytes, representing a Bytes32 hash
type Bytes32 = Vector<u8, 32>;
/// A type alias for a vector of 20 bytes, representing an address
type VecAddress = Vector<u8, 20>;
/// A type alias for a byte list, representing a transaction
type Transaction = List<u8, 1073741824>;

/// The pre Canyon/Shanghai [ExecutionPayload] - the withdrawals field should not exist
#[derive(SimpleSerialize, Default)]
struct ExecutionPayloadV1SSZ {
    /// Block hash of the parent block
    pub parent_hash: Bytes32,
    /// Fee recipient of the block. Set to the sequencer fee vault
    pub fee_recipient: VecAddress,
    /// State root of the block
    pub state_root: Bytes32,
    /// Receipts root of the block
    pub receipts_root: Bytes32,
    /// Logs bloom of the block
    pub logs_bloom: Vector<u8, 256>,
    /// The block mix_digest
    pub prev_randao: Bytes32,
    /// The block number
    pub block_number: u64,
    /// The block gas limit
    pub gas_limit: u64,
    /// Total gas used in the block
    pub gas_used: u64,
    /// Timestamp of the block
    pub timestamp: u64,
    /// Any extra data included in the block
    pub extra_data: List<u8, 32>,
    /// Base fee per gas of the block
    pub base_fee_per_gas: U256,
    /// Hash of the block
    pub block_hash: Bytes32,
    /// Transactions in the block
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
            withdrawals: None,
            blob_gas_used: None,
            excess_blob_gas: None,
        }
    }
}

/// The Canyon/Shanghai [ExecutionPayload] - the withdrawals field should be an empty [List]
#[derive(SimpleSerialize, Default)]
struct ExecutionPayloadV2SSZ {
    /// Block hash of the parent block
    pub parent_hash: Bytes32,
    /// Fee recipient of the block. Set to the sequencer fee vault
    pub fee_recipient: VecAddress,
    /// State root of the block
    pub state_root: Bytes32,
    /// Receipts root of the block
    pub receipts_root: Bytes32,
    /// Logs bloom of the block
    pub logs_bloom: Vector<u8, 256>,
    /// The block mix_digest
    pub prev_randao: Bytes32,
    /// The block number
    pub block_number: u64,
    /// The block gas limit
    pub gas_limit: u64,
    /// Total gas used in the block
    pub gas_used: u64,
    /// Timestamp of the block
    pub timestamp: u64,
    /// Any extra data included in the block
    pub extra_data: List<u8, 32>,
    /// Base fee per gas of the block
    pub base_fee_per_gas: U256,
    /// Hash of the block
    pub block_hash: Bytes32,
    /// Transactions in the block
    pub transactions: List<Transaction, 1048576>,
    /// An empty list. This is unused and only exists for L1 compatibility.
    pub withdrawals: List<Withdrawal, 16>,
}

/// This represents an L1 validator Withdrawal, and is unused in OP stack rollups.
/// Exists only for L1 compatibility
#[derive(SimpleSerialize, Default)]
struct Withdrawal {
    /// Index of the withdrawal
    index: u64,
    /// Index of the validator
    validator_index: u64,
    /// Account address that has withdrawn
    address: VecAddress,
    /// The amount withdrawn
    amount: u64,
}

impl From<ExecutionPayloadV2SSZ> for ExecutionPayload {
    /// Converts an ExecutionPayloadV2SSZ received via p2p gossip into an [ExecutionPayload] used by the engine.
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
            withdrawals: Some(Vec::new()),
            blob_gas_used: None,
            excess_blob_gas: None,
        }
    }
}

#[derive(SimpleSerialize, Default)]
struct ExecutionPayloadV3SSZ {
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
    pub blob_gas_used: u64,
    pub excess_blob_gas: u64,
}

impl From<ExecutionPayloadV3SSZ> for ExecutionPayload {
    fn from(value: ExecutionPayloadV3SSZ) -> Self {
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
            withdrawals: Some(Vec::new()),
            blob_gas_used: Some(value.blob_gas_used.into()),
            excess_blob_gas: Some(value.excess_blob_gas.into()),
        }
    }
}

/// Converts [Bytes32] into [H256]
fn convert_hash(bytes: Bytes32) -> B256 {
    B256::from_slice(bytes.as_slice())
}

/// Converts [VecAddress] into [Address]
fn convert_address(address: VecAddress) -> Address {
    Address::from_slice(address.as_slice())
}

/// Converts an [ssz_rs::Vector] of bytes into [Bytes]
fn convert_byte_vector<const N: usize>(vector: Vector<u8, N>) -> Bytes {
    Bytes::from(vector.to_vec())
}

/// Converts an [ssz_rs::List] of bytes into [Bytes]
fn convert_byte_list<const N: usize>(list: List<u8, N>) -> Bytes {
    Bytes::from(list.to_vec())
}

/// Converts a [U256] into [ethers::types::U64]
fn convert_uint(value: U256) -> ethers::types::U64 {
    let bytes = value.to_bytes_le();
    ethers::types::U256::from_little_endian(&bytes)
        .as_u64()
        .into()
}

/// Converts [ssz_rs::List] of [Transaction] into a vector of [RawTransaction]
fn convert_tx_list(value: List<Transaction, 1048576>) -> Vec<RawTransaction> {
    value.iter().map(|tx| RawTransaction(tx.to_vec())).collect()
}
