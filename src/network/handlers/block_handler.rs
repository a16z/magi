use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::SystemTime;

use ethers::types::{Address, Bytes, Signature, H160, H256};
use ethers::utils::keccak256;

use eyre::Result;
use libp2p::gossipsub::{IdentTopic, Message, MessageAcceptance, TopicHash};
use ssz_rs::{prelude::*, List, Vector, U256};
use tokio::sync::watch;

use crate::network::signer::Signer;
use crate::{engine::ExecutionPayload, types::attributes::RawTransaction};

use super::Handler;

pub struct BlockHandler {
    chain_id: u64,
    block_sender: Sender<ExecutionPayload>,
    unsafe_signer_recv: watch::Receiver<Address>,
    blocks_v1_topic: IdentTopic,
    blocks_v2_topic: IdentTopic,
}

impl Handler for BlockHandler {
    fn handle(&self, msg: Message) -> MessageAcceptance {
        tracing::debug!("received block");

        let decoded = if msg.topic == self.blocks_v1_topic.hash() {
            decode_block_msg::<ExecutionPayloadV1SSZ>(msg.data)
        } else if msg.topic == self.blocks_v2_topic.hash() {
            decode_block_msg::<ExecutionPayloadV2SSZ>(msg.data)
        } else {
            return MessageAcceptance::Reject;
        };

        match decoded {
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

    fn topics(&self) -> Vec<TopicHash> {
        vec![self.blocks_v1_topic.hash(), self.blocks_v2_topic.hash()]
    }
}

impl BlockHandler {
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

    // TODO: Seems it can panic, what's basically can crash node.
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
            withdrawals: None,
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
            withdrawals: Some(Vec::new()),
        }
    }
}

impl TryFrom<ExecutionPayload> for ExecutionPayloadV2SSZ {
    type Error = eyre::Report;

    fn try_from(value: ExecutionPayload) -> Result<Self> {
        Ok(Self {
            parent_hash: convert_hash_to_bytes32(value.parent_hash)?,
            fee_recipient: convert_hash_to_address(value.fee_recipient)?,
            state_root: convert_hash_to_bytes32(value.state_root)?,
            receipts_root: convert_hash_to_bytes32(value.receipts_root)?,
            logs_bloom: convert_bytes_to_vector(value.logs_bloom)?,
            prev_randao: convert_hash_to_bytes32(value.prev_randao)?,
            block_number: value.block_number.as_u64(),
            gas_limit: value.gas_limit.as_u64(),
            gas_used: value.gas_used.as_u64(),
            timestamp: value.timestamp.as_u64(),
            extra_data: convert_bytes_to_list(value.extra_data)?,
            base_fee_per_gas: value.base_fee_per_gas.as_u64().into(),
            block_hash: convert_hash_to_bytes32(value.block_hash)?,
            transactions: convert_tx_to_list(value.transactions)?,
            withdrawals: List::default(),
        })
    }
}

fn convert_hash_to_bytes32(hash: H256) -> Result<Bytes32> {
    Bytes32::try_from(hash.as_fixed_bytes().to_vec())
        .map_err(|_| eyre::eyre!("can't convert H256 to Bytes32"))
}

fn convert_hash_to_address(hash: H160) -> Result<VecAddress> {
    VecAddress::try_from(hash.as_fixed_bytes().to_vec())
        .map_err(|_| eyre::eyre!("can't convert H160 to Address"))
}

fn convert_bytes_to_list(data: Bytes) -> Result<List<u8, 32>> {
    List::<u8, 32>::try_from(data.to_vec())
        .map_err(|_| eyre::eyre!("can't convert bytes to List 32 size"))
}

fn convert_bytes_to_vector(data: Bytes) -> Result<Vector<u8, 256>> {
    Vector::<u8, 256>::try_from(data.to_vec())
        .map_err(|_| eyre::eyre!("can't convert bytes to Vector 256 size"))
}

fn convert_tx_to_list(txs: Vec<RawTransaction>) -> Result<List<Transaction, 1048576>> {
    let mut list: List<Transaction, 1048576> = Default::default();

    for tx in txs {
        let list_tx = Transaction::try_from(tx.0)
            .map_err(|_| eyre::eyre!("can't convert RawTransaction to List"))?;
        list.push(list_tx);
    }

    Ok(list)
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

pub fn encode_block_msg(payload: ExecutionPayload, signer: &Signer) -> Result<Vec<u8>> {
    // Start preparing payload for distribution.
    let payload_ssz: ExecutionPayloadV2SSZ = payload.try_into()?;
    let payload_bytes = serialize(&payload_ssz)?;

    // Signature.
    let (_, sig) = signer.sign(&payload_bytes)?;

    // Create a payload for distribution.
    let mut data: Vec<u8> = vec![];
    data.extend(sig);
    data.extend(payload_bytes);

    // Zip.
    let mut encoder = snap::raw::Encoder::new();

    // The value can be passed by P2P.
    Ok(encoder.compress_vec(&data)?)
}

#[cfg(test)]
mod tests {
    use crate::{
        engine::ExecutionPayload, network::signer::Signer, types::attributes::RawTransaction,
    };
    use ethers::core::k256::ecdsa::SigningKey;
    use ethers::types::{Bytes, H160, H256, U64};
    use ssz_rs::prelude::*;

    use eyre::Result;

    use rand::Rng;

    use super::{decode_block_msg, encode_block_msg, ExecutionPayloadV2SSZ};

    #[test]
    fn test_prepare_payload() -> Result<()> {
        let mut rng = rand::thread_rng();
        let tx = RawTransaction(rng.gen::<[u8; 32]>().to_vec());

        let mut logs_bloom = [0u8; 256];
        rng.fill(&mut logs_bloom);

        let payload = ExecutionPayload {
            parent_hash: H256::random(),
            fee_recipient: H160::random(),
            state_root: H256::random(),
            receipts_root: H256::random(),
            logs_bloom: Bytes::from(logs_bloom),
            prev_randao: H256::random(),
            block_number: U64::from(rng.gen::<u64>()),
            gas_limit: U64::from(rng.gen::<u64>()),
            gas_used: U64::from(rng.gen::<u64>()),
            timestamp: U64::from(rng.gen::<u64>()),
            extra_data: Bytes::from(rng.gen::<[u8; 32]>()),
            base_fee_per_gas: U64::from(rng.gen::<u64>()),
            block_hash: H256::random(),
            transactions: vec![tx],
            withdrawals: Some(vec![]),
        };

        // Start preparing payload for distribution.
        let payload_ssz: ExecutionPayloadV2SSZ = payload.clone().try_into()?;
        let payload_bytes = serialize(&payload_ssz)?;

        // Sign.
        let private_key = SigningKey::random(&mut rand::thread_rng());
        let signer = Signer::new(1, private_key, None)?;

        // Signature.
        let (_, sig) = signer.sign(&payload_bytes)?;

        // Create a payload for distribution.
        let mut data: Vec<u8> = vec![];
        data.extend(sig.clone());
        data.extend(payload_bytes);

        // Zip.
        let mut encoder = snap::raw::Encoder::new();

        // The value can be passed by P2P.
        let compressed_1 = encoder.compress_vec(&data)?;
        let compressed_2 = encode_block_msg(payload.clone(), &signer)?;

        assert_eq!(compressed_1, compressed_2);

        for tx in [compressed_1, compressed_2] {
            let (decoded_payload, decoded_signature, _) =
                decode_block_msg::<ExecutionPayloadV2SSZ>(tx)?;
            assert_eq!(payload, decoded_payload, "decoded payload different");
            assert_eq!(
                &sig,
                &decoded_signature.to_vec(),
                "decoded signature different"
            );
        }
        Ok(())
    }
}
