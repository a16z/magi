use ethers::types::{Block, Bytes, Transaction, H160, H256, U64};
use eyre::Result;
use serde::{Deserialize, Serialize};

use crate::{
    common::{Epoch, RawTransaction},
    config::SystemAccounts,
};

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
    pub logs_bloom: Bytes,
    /// A 32 byte beacon chain randomness value
    pub prev_randao: H256,
    /// A 64 bit number for the current block index
    pub block_number: U64,
    /// A 64 bit value for the gas limit
    pub gas_limit: U64,
    /// A 64 bit value for the gas used
    pub gas_used: U64,
    /// A 64 bit value for the timestamp field of the new payload
    pub timestamp: U64,
    /// 0 to 32 byte value for extra data
    pub extra_data: Bytes,
    /// 256 bits for the base fee per gas
    pub base_fee_per_gas: U64,
    /// The 32 byte block hash
    pub block_hash: H256,
    /// An array of transaction objects where each object is a byte list
    pub transactions: Vec<RawTransaction>,
}

impl TryFrom<Block<Transaction>> for ExecutionPayload {
    type Error = eyre::Report;

    fn try_from(value: Block<Transaction>) -> Result<Self> {
        let encoded_txs = (*value
            .transactions
            .into_iter()
            .map(|tx| RawTransaction(tx.rlp().to_vec()))
            .collect::<Vec<_>>())
        .to_vec();

        Ok(ExecutionPayload {
            parent_hash: value.parent_hash,
            fee_recipient: value.author.unwrap_or(SystemAccounts::default().fee_vault),
            state_root: value.state_root,
            receipts_root: value.receipts_root,
            logs_bloom: value.logs_bloom.unwrap().as_bytes().to_vec().into(),
            prev_randao: value.mix_hash.unwrap(),
            block_number: value.number.unwrap(),
            gas_limit: value.gas_limit.as_u64().into(),
            gas_used: value.gas_used.as_u64().into(),
            timestamp: value.timestamp.as_u64().into(),
            extra_data: value.extra_data.clone(),
            base_fee_per_gas: value
                .base_fee_per_gas
                .unwrap_or_else(|| 0u64.into())
                .as_u64()
                .into(),
            block_hash: value.hash.unwrap(),
            transactions: encoded_txs,
        })
    }
}

/// ## PayloadAttributes
///
/// L2 extended payload attributes for Optimism.
/// For more details, visit the [Optimism specs](https://github.com/ethereum-optimism/optimism/blob/develop/specs/exec-engine.md#extended-payloadattributesv1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PayloadAttributes {
    /// 64 bit value for the timestamp field of the new payload.
    pub timestamp: U64,
    /// 32 byte value for the prevRandao field of the new payload.
    pub prev_randao: H256,
    ///  20 bytes suggested value for the feeRecipient field of the new payload.
    pub suggested_fee_recipient: H160,
    /// Array of transactions to be included in the new payload.
    pub transactions: Option<Vec<RawTransaction>>,
    /// Boolean value indicating whether or not the payload should be built without including transactions from the txpool.
    pub no_tx_pool: bool,
    /// 64 bit value for the gasLimit field of the new payload.
    /// The gasLimit is optional w.r.t. compatibility with L1, but required when used as rollup.
    /// This field overrides the gas limit used during block-building.
    /// If not specified as rollup, a STATUS_INVALID is returned.
    pub gas_limit: U64,
    /// The batch epoch number from derivation. This value is not expected by the engine is skipped
    /// during serialization and deserialization.
    #[serde(skip)]
    pub epoch: Option<Epoch>,
    /// The L1 block number when this batch was first fully derived. This value is not expected by
    /// the engine and is skipped during serialization and deserialization.
    #[serde(skip)]
    pub l1_inclusion_block: Option<u64>,
    /// The L2 sequence number of the block. This value is not expected by the engine and is
    /// skipped during serialization and deserialization.
    #[serde(skip)]
    pub seq_number: Option<u64>,
}

/// ## PayloadId
pub type PayloadId = U64;

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
    #[serde(default)]
    pub validation_error: Option<String>,
}

/// ## Status
///
/// The status of the payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Status {
    /// Valid Payload
    Valid,
    /// Invalid Payload
    Invalid,
    /// Currently syncing
    Syncing,
    /// Payload is accepted
    Accepted,
    /// Payload contains an invalid block hash
    InvalidBlockHash,
}

#[cfg(test)]
mod tests {

    use ethers::{
        providers::{Http, Middleware, Provider},
        types::H256,
    };
    use eyre::Result;

    use crate::engine::ExecutionPayload;

    #[tokio::test]
    async fn test_from_block_hash_to_execution_paylaod() -> Result<()> {
        if std::env::var("L1_TEST_RPC_URL").is_ok() && std::env::var("L2_TEST_RPC_URL").is_ok() {
            let checkpoint_hash: H256 =
                "0xc2794a16acacd9f7670379ffd12b6968ff98e2a602f57d7d1f880220aa5a4973".parse()?;

            let l2_rpc = std::env::var("L2_TEST_RPC_URL")?;
            let checkpoint_sync_url = Provider::<Http>::try_from(l2_rpc)?;
            let checkpoint_block = checkpoint_sync_url
                .get_block_with_txs(checkpoint_hash)
                .await?
                .unwrap();

            let payload = ExecutionPayload::try_from(checkpoint_block)?;

            assert_eq!(
                payload.block_hash,
                "0xc2794a16acacd9f7670379ffd12b6968ff98e2a602f57d7d1f880220aa5a4973".parse()?
            );
            assert_eq!(payload.block_number, 8453214u64.into());
            assert_eq!(payload.base_fee_per_gas, 50u64.into());
        }

        Ok(())
    }
}
