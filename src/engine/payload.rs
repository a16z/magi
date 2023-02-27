use ethers_core::types::{H160, H256};
use serde::{Deserialize, Serialize};

/// ## ExecutionPayload
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionPayload {
    /// A 32 byte hash of the parent payload
    pub parent_hash: H256,
    /// A 20 byte hash (aka Address) for the feeRecipient field of the new payload
    pub fee_recipient: H160,
    /// A 32 byte state root hash
    pub state_root: H256,
    /// A 32 byte receipt root hash
    pub receipts_root: H256,
    /// A 32 byte logs bloom filter
    pub logs_bloom: Vec<u8>,
    /// A 32 byte beacon chain randomness value
    pub prev_randao: H256,
    /// A 64 bit number for the current block index
    pub block_number: u64,
    /// A 64 bit value for the gas limit
    pub gas_limit: u64,
    /// A 64 bit value for the gas used
    pub gas_used: u64,
    /// A 64 bit value for the timestamp field of the new payload
    pub timestamp: u64,
    /// 0 to 32 byte value for extra data
    pub extra_data: Vec<u8>,
    /// 256 bits for the base fee per gas
    pub base_fee_per_gas: u64,
    /// The 32 byte block hash
    pub block_hash: H256,
    /// An array of transaction objects where each object is a byte list
    pub transactions: Vec<Vec<u8>>,
}

/// L1 PayloadAttributes
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct L1PayloadAttributes {
    /// 64 bit value for the timestamp field of the new payload
    pub timestamp: u64,
    /// 32 byte value for the prevRandao field of the new payload
    pub prev_randao: H256,
    ///  20 bytes suggested value for the feeRecipient field of the new payload
    pub suggested_fee_recipient: H160,
}

/// ## PayloadAttributes
///
/// L2 extended payload attributes for Optimism.
/// For more details, visit the [Optimism specs](https://github.com/ethereum-optimism/optimism/blob/develop/specs/exec-engine.md#extended-payloadattributesv1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PayloadAttributes {
    /// 64 bit value for the timestamp field of the new payload.
    pub timestamp: u64,
    /// 32 byte value for the prevRandao field of the new payload.
    pub prev_randao: H256,
    ///  20 bytes suggested value for the feeRecipient field of the new payload.
    pub suggested_fee_recipient: H160,
    /// Array of transactions to be included in the new payload.
    pub transactions: Option<Vec<Vec<u8>>>,
    /// Boolean value indicating whether or not the payload should be built without including transactions from the txpool.
    pub no_tx_pool: bool,
    /// 64 bit value for the gasLimit field of the new payload.
    /// The gasLimit is optional w.r.t. compatibility with L1, but required when used as rollup.
    /// This field overrides the gas limit used during block-building.
    /// If not specified as rollup, a STATUS_INVALID is returned.
    pub gas_limit: u64,
}

/// ## PayloadId
pub type PayloadId = u64;

/// ## PayloadStatus
///
/// The status of a payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PayloadStatus {
    /// The status of the payload.
    pub status: Status,
    /// 32 Bytes - the hash of the most recent valid block in the branch defined by payload and its ancestors
    pub latest_valid_hash: Option<H256>,
    /// A message providing additional details on the validation error if the payload is classified as INVALID or INVALID_BLOCK_HASH.
    pub validation_error: Option<String>,
}

/// ## Status
///
/// The status of the payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Status {
    Valid,
    Invalid,
    Syncing,
    Accepted,
    InvalidBlockHash,
}
