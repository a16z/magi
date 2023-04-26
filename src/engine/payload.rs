use ethers::{
    providers::{Http, Middleware, Provider},
    types::{BlockId, Bytes, H160, H256, U64},
};
use eyre::Result;
use serde::{Deserialize, Serialize};

use crate::{
    common::{Epoch, RawTransaction},
    config::Config,
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

impl ExecutionPayload {
    /// ## from_block
    ///
    /// Creates a new ExecutionPayload from a block hash.
    /// Requires both an L1 rpc url and a trusted L2 rpc url.
    /// Ported from [op-fast-sync](https://github.com/testinprod-io/op-fast-sync/blob/master/build_payloads.py)
    pub async fn from_block(config: &Config, block_hash: H256) -> Result<Self> {
        let l1_provider = Provider::<Http>::try_from(config.l1_rpc_url.as_str())?;
        let l2_provider = Provider::<Http>::try_from(
            config
                .l2_trusted_rpc_url
                .clone()
                .expect("trusted l2 rpc url is required to build a payload from a block hash")
                .as_str(),
        )?;

        let l2_block = l2_provider
            .get_block_with_txs(block_hash)
            .await?
            .expect("l2 block not found");

        let l1_block_number_raw = l2_provider
            .get_storage_at(
                config.chain.l1_block,
                H256::zero(),
                Some(BlockId::Number(l2_block.number.unwrap().into())),
            )
            .await?;

        let l1_block_number_raw = &format!("{:x}", l1_block_number_raw);
        let l1_block_number_raw = &l1_block_number_raw[48..];
        let l1_block_number = U64::from_str_radix(l1_block_number_raw, 16)?;

        let l1_block = l1_provider
            .get_block(l1_block_number)
            .await?
            .expect("l1 block not found");

        let txs = (*l2_block
            .transactions
            .clone()
            .into_iter()
            .map(|tx| RawTransaction(tx.rlp().to_vec()))
            .collect::<Vec<_>>())
        .to_vec();

        Ok(ExecutionPayload {
            parent_hash: l2_block.parent_hash,
            fee_recipient: config.chain.sequencer_fee_vault,
            state_root: l2_block.state_root,
            receipts_root: l2_block.receipts_root,
            logs_bloom: l2_block.logs_bloom.unwrap().as_bytes().to_vec().into(),
            prev_randao: l1_block.mix_hash.unwrap(),
            block_number: l2_block.number.unwrap(),
            gas_limit: l2_block.gas_limit.as_u64().into(),
            gas_used: l2_block.gas_used.as_u64().into(),
            timestamp: l2_block.timestamp.as_u64().into(),
            extra_data: l2_block.extra_data.clone(),
            base_fee_per_gas: l2_block
                .base_fee_per_gas
                .unwrap_or(0u64.into())
                .as_u64()
                .into(),
            block_hash: l2_block.hash.unwrap(),
            transactions: txs,
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
    use std::sync::Arc;

    use eyre::Result;

    use crate::{
        config::{ChainConfig, Config},
        engine::ExecutionPayload,
    };

    #[tokio::test]
    async fn test_from_block_hash_to_execution_paylaod() -> Result<()> {
        let checkpoint_hash =
            "0xc2794a16acacd9f7670379ffd12b6968ff98e2a602f57d7d1f880220aa5a4973".parse()?;

        let rpc = "https://eth-goerli.g.alchemy.com/v2/UbmnU8fj4rLikYW5ph8Xe975Pz-nxqfv".to_owned();
        let l2_rpc =
            "https://opt-goerli.g.alchemy.com/v2/UbmnU8fj4rLikYW5ph8Xe975Pz-nxqfv".to_owned();
        let config = Arc::new(Config {
            l1_rpc_url: rpc,
            l2_rpc_url: l2_rpc.clone(),
            chain: ChainConfig::optimism_goerli(),
            l2_engine_url: String::new(),
            jwt_secret: String::new(),
            l2_trusted_rpc_url: Some(l2_rpc),
            rpc_port: 0,
        });

        let payload = ExecutionPayload::from_block(&config, checkpoint_hash).await?;

        assert_eq!(
            payload.block_hash,
            "0xc2794a16acacd9f7670379ffd12b6968ff98e2a602f57d7d1f880220aa5a4973".parse()?
        );
        assert_eq!(payload.block_number, 8453214u64.into());
        assert_eq!(payload.base_fee_per_gas, 50u64.into());

        Ok(())
    }
}
