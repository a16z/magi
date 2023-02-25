use ethers_core::types::{Transaction, H160, H256};
use serde::{Deserialize, Serialize};

/// ## ExecutionPayloadV1
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionPayload {
    pub parent_hash: H256,
    pub fee_recipient: H256,
    pub state_root: H256,
    pub receipts_root: H256,
    pub logs_bloom: Vec<u8>,
    pub prev_randao: H256,
    pub block_number: u64,
    pub gas_limit: u64,
    pub gas_used: u64,
    pub timestamp: u64,
    pub extra_data: Vec<u8>,
    pub base_fee_per_gas: u64,
    pub block_hash: H256,
    pub transactions: Vec<Transaction>,
}

/// L1 PayloadAttributesV1
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

/// PayloadAttributesV1
///
/// L2 extended payload attributes for Optimism.
/// For more details, visit the [Optimism specs](https://github.com/ethereum-optimism/optimism/blob/develop/specs/exec-engine.md#extended-payloadattributesv1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PayloadAttributes {
    /// 64 bit value for the timestamp field of the new payload.
    pub timestamp: u64,
    /// 32 byte value for the prevRandao field of the new payload.
    pub prev_randao: H256,
    ///  20 bytes suggested value for the feeRecipient field of the new payload.
    pub suggested_fee_recipient: H160,
    /// Array of transactions to be included in the new payload.
    pub transactions: Option<Vec<Transaction>>,
    /// Boolean value indicating whether or not the payload should be built without including transactions from the txpool.
    pub no_tx_pool: bool,
    /// 64 bit value for the gasLimit field of the new payload.
    /// The gasLimit is optional w.r.t. compatibility with L1, but required when used as rollup.
    /// This field overrides the gas limit used during block-building.
    /// If not specified as rollup, a STATUS_INVALID is returned.
    pub gas_limit: u64,
}

/// ## PayloadIdV1
pub type PayloadId = [u8; 8];

/// ## PayloadStatusV1
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
