use eyre::Result;
use serde::{Deserialize, Serialize};
use alloy_consensus::TxEnvelope;
use alloy_eips::eip2718::Encodable2718;
use alloy_rpc_types::{Block, BlockTransactions, Transaction};
use alloy_primitives::{Bytes, Address, B256, U64};

use crate::{
    common::{Epoch, RawTransaction},
    config::SystemAccounts,
};

/// ## ExecutionPayload
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionPayload {
    /// A 32 byte hash of the parent payload
    pub parent_hash: B256,
    /// A 20 byte hash (aka Address) for the feeRecipient field of the new payload
    pub fee_recipient: Address,
    /// A 32 byte state root hash
    pub state_root: B256,
    /// A 32 byte receipt root hash
    pub receipts_root: B256,
    /// A 32 byte logs bloom filter
    pub logs_bloom: Bytes,
    /// A 32 byte beacon chain randomness value
    pub prev_randao: B256,
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
    pub block_hash: B256,
    /// An array of transaction objects where each object is a byte list
    pub transactions: Vec<RawTransaction>,
    /// An array of beaconchain withdrawals. Always empty as this exists only for L1 compatibility
    #[serde(skip_serializing_if = "Option::is_none")]
    pub withdrawals: Option<Vec<()>>,
    /// None if not present (pre-Ecotone)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob_gas_used: Option<U64>,
    /// None if not present (pre-Ecotone)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excess_blob_gas: Option<U64>,
}

impl TryFrom<Block<Transaction>> for ExecutionPayload {
    type Error = eyre::Report;

    /// Converts a [Block] to an [ExecutionPayload]
    fn try_from(value: Block<Transaction>) -> Result<Self> {
        let txs = match value.transactions {
            BlockTransactions::Full(txs) => txs,
            _ => return Err(eyre::eyre!("Invalid block transactions")),
        };
        let mut encoded = Vec::with_capacity(txs.len());
        for tx in txs {
            let envelope = TxEnvelope::try_from(tx)?;
            let mut by = vec![];
            envelope.encode_2718(&mut by);
            encoded.push(RawTransaction(by));
        }

        Ok(ExecutionPayload {
            parent_hash: value.header.parent_hash,
            fee_recipient: Address::from_slice(
                SystemAccounts::default().fee_vault.as_slice(),
            ),
            state_root: value.header.state_root,
            receipts_root: value.header.receipts_root,
            logs_bloom: value.header.logs_bloom.as_slice().to_vec().into(),
            prev_randao: value.header.mix_hash.unwrap(),
            block_number: value.header.number.unwrap_or_default().try_into()?,
            gas_limit: value.header.gas_limit.try_into()?,
            gas_used: value.header.gas_used.try_into()?,
            timestamp: value.header.timestamp.try_into()?,
            extra_data: value.header.extra_data.clone(),
            base_fee_per_gas: value
                .header
                .base_fee_per_gas
                .unwrap_or_default()
                .try_into()?,
            block_hash: value.header.hash.unwrap(),
            transactions: encoded,
            withdrawals: Some(Vec::new()),
            blob_gas_used: value.header.blob_gas_used.map(|v| v.try_into()).transpose()?,
            excess_blob_gas: value.header.excess_blob_gas.map(|v| v.try_into()).transpose()?,
        })
    }
}

impl TryFrom<ethers::types::Block<ethers::types::Transaction>> for ExecutionPayload {
    type Error = eyre::Report;

    /// Converts a [Block] to an [ExecutionPayload]
    fn try_from(value: ethers::types::Block<ethers::types::Transaction>) -> Result<Self> {
        let encoded_txs = (*value
            .transactions
            .into_iter()
            .map(|tx| RawTransaction(tx.rlp().to_vec()))
            .collect::<Vec<_>>())
        .to_vec();

        Ok(ExecutionPayload {
            parent_hash: value.parent_hash,
            fee_recipient: ethers::types::Address::from_slice(
                SystemAccounts::default().fee_vault.as_slice(),
            ),
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
            withdrawals: Some(Vec::new()),
            blob_gas_used: value.blob_gas_used.map(|v| v.as_u64().into()),
            excess_blob_gas: value.excess_blob_gas.map(|v| v.as_u64().into()),
        })
    }
}

/// ## PayloadAttributes
///
/// L2 extended payload attributes for Optimism.
/// For more details, visit the [Optimism specs](https://github.com/ethereum-optimism/specs/blob/main/specs/protocol/exec-engine.md#extended-payloadattributesv1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PayloadAttributes {
    /// 64 bit value for the timestamp field of the new payload.
    pub timestamp: U64,
    /// 32 byte value for the prevRandao field of the new payload.
    pub prev_randao: B256,
    ///  20 bytes suggested value for the feeRecipient field of the new payload.
    pub suggested_fee_recipient: Address,
    /// Array of transactions to be included in the new payload.
    pub transactions: Option<Vec<RawTransaction>>,
    /// Boolean value indicating whether or not the payload should be built without including transactions from the txpool.
    pub no_tx_pool: bool,
    /// 64 bit value for the gasLimit field of the new payload.
    /// The gasLimit is optional w.r.t. compatibility with L1, but required when used as rollup.
    /// This field overrides the gas limit used during block-building.
    /// If not specified as rollup, a STATUS_INVALID is returned.
    pub gas_limit: U64,
    /// Beaconchain withdrawals. This exists only for compatibility with L1, and is not used. Prior
    /// to Canyon, this value is always None. After Canyon it is an empty array. Note that we use
    /// the () type here since we never have a non empty array.
    pub withdrawals: Option<Vec<()>>,
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
    pub latest_valid_hash: Option<B256>,
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

    use eyre::Result;
    use reqwest::Url;
    use alloy_primitives::{uint, b256};
    use alloy_network_primitives::BlockTransactionsKind;
    use alloy_provider::{network::Ethereum, Provider, ReqwestProvider};

    use crate::engine::ExecutionPayload;

    #[tokio::test]
    async fn test_from_block_hash_to_execution_paylaod() -> Result<()> {
        if std::env::var("L2_TEST_RPC_URL").is_ok() {
            let checkpoint_hash =
                b256!("c2794a16acacd9f7670379ffd12b6968ff98e2a602f57d7d1f880220aa5a4973");

            let l2_rpc = std::env::var("L2_TEST_RPC_URL")?;
            let l2_rpc_url = Url::parse(&l2_rpc)?;
            let checkpoint_sync_url = ReqwestProvider::<Ethereum>::new_http(l2_rpc_url);
            let checkpoint_block = checkpoint_sync_url
                .get_block(checkpoint_hash.into(), BlockTransactionsKind::Full)
                .await?
                .unwrap();

            let payload = ExecutionPayload::try_from(checkpoint_block)?;

            assert_eq!(
                payload.block_hash,
                b256!("c2794a16acacd9f7670379ffd12b6968ff98e2a602f57d7d1f880220aa5a4973")
            );
            assert_eq!(payload.block_number, uint!(8453214_U64));
            assert_eq!(payload.base_fee_per_gas, uint!(50_U64));
        }

        Ok(())
    }
}
