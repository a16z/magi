//! A module to handle fetching blocks.

use crate::config::Config;
use crate::driver::types::HeadInfo;

use alloy_provider::Provider;
use alloy_rpc_types::{Block, BlockId, BlockNumberOrTag};
use eyre::Result;

/// An asynchronous trait for fetching blocks along with their transactions.
#[async_trait::async_trait]
pub trait InnerProvider {
    /// Retrieves a block and its transactions
    async fn get_block_with_txs(&self, block_id: BlockId) -> Result<Option<Block>>;
}

/// Wrapper around a [ReqwestProvider].
pub struct HeadInfoFetcher<'a> {
    inner: &'a dyn Provider,
}

impl<'a> From<&'a dyn Provider> for HeadInfoFetcher<'a> {
    /// Converts a [ReqwestProvider] to a [HeadInfoFetcher].
    fn from(inner: &'a dyn Provider) -> Self {
        Self { inner }
    }
}

#[async_trait::async_trait]
impl<'a> InnerProvider for HeadInfoFetcher<'a> {
    /// Fetches a [Block] with transactions.
    async fn get_block_with_txs(&self, block_id: BlockId) -> Result<Option<Block>> {
        self.inner
            .get_block(block_id, true)
            .await
            .map_err(Into::into)
    }
}

/// Provides a method to fetch the latest finalized [Block].
pub struct HeadInfoQuery {}

impl HeadInfoQuery {
    /// Fetches the latest finalized L2 block
    pub async fn get_head_info<P: InnerProvider>(p: impl Provider, config: &Config) -> HeadInfo {
        let parsed_head_info = match p
            .get_block(BlockId::Number(BlockNumberOrTag::Finalized), true)
            .await
        {
            Ok(Some(block)) => match HeadInfo::try_from_l2_block(config, block) {
                Ok(head_info) => Some(head_info),
                Err(e) => {
                    tracing::debug!(err = ?e, "could not parse L2 block into head info");
                    None
                }
            },
            e => {
                tracing::debug!("could not get finalied L2 block: {:?}", e);
                None
            }
        };

        if let Some(head_info) = parsed_head_info {
            head_info
        } else {
            tracing::warn!("could not get head info. Falling back to the genesis head.");
            HeadInfo {
                l2_block_info: config.chain.l2_genesis,
                l1_epoch: config.chain.l1_start_epoch,
                sequence_number: 0,
            }
        }
    }
}

#[cfg(all(test, feature = "test-utils"))]
mod test_utils {
    use super::*;
    use crate::common::{BlockInfo, Epoch};
    use crate::config::{ChainConfig, Config};
    use alloy_primitives::b256;

    pub struct MockProvider {
        pub block: Option<Block>,
    }

    pub fn mock_provider(block: Option<Block>) -> MockProvider {
        MockProvider { block }
    }

    pub fn default_head_info() -> HeadInfo {
        HeadInfo {
            l2_block_info: BlockInfo {
                hash: b256!("dbf6a80fef073de06add9b0d14026d6e5a86c85f6d102c36d3d8e9cf89c2afd3"),
                number: 105235063,
                parent_hash: b256!(
                    "21a168dfa5e727926063a28ba16fd5ee84c814e847c81a699c7a0ea551e4ca50"
                ),
                timestamp: 1686068903,
            },
            l1_epoch: Epoch {
                number: 17422590,
                hash: b256!("438335a20d98863a4c0c97999eb2481921ccd28553eac6f913af7c12aec04108"),
                timestamp: 1686068903,
            },
            sequence_number: 0,
        }
    }

    pub fn valid_block() -> Option<Block> {
        let raw_block = r#"{
            "hash": "0x2e4f4aff36bb7951be9742ad349fb1db84643c6bbac5014f3d196fd88fe333eb",
            "parentHash": "0xeccf4c06ad0d27be1cadee5720a509d31a9de0462b52f2cf6045d9a73c9aa504",
            "sha3Uncles": "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
            "miner": "0x4200000000000000000000000000000000000011",
            "stateRoot": "0x5905b2423f299a29db41e377d7ceadf4baa49eed04e1b72957e8c0985e04e730",
            "transactionsRoot": "0x030e481411042a769edde83d790d583ed69f9d3098d4a78d00e008f749fcfd97",
            "receiptsRoot": "0x29079b696c12a19999f3bb303fddb6fc12fb701f427678cca24954b91080ada3",
            "number": "0x7fe52f",
            "gasUsed": "0xb711",
            "gasLimit": "0x17d7840",
            "extraData": "0x",
            "logsBloom": "0x00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            "timestamp": "0x644434c2",
            "difficulty": "0x0",
            "totalDifficulty": "0x0",
            "sealFields": [],
            "uncles": [],
            "transactions": [],
            "size": "0x365",
            "mixHash": "0x7aeec5550a9b0616701e49ab835af5f10eadba2a0582016f0e256c9cace0c046",
            "nonce": "0x0000000000000000",
            "baseFeePerGas": "0x32"
        }
        "#;
        serde_json::from_str(raw_block).ok()
    }

    pub fn optimism_config() -> Config {
        Config {
            l1_rpc_url: Default::default(),
            l1_beacon_url: Default::default(),
            l2_rpc_url: Default::default(),
            l2_engine_url: Default::default(),
            chain: ChainConfig::optimism(),
            jwt_secret: Default::default(),
            checkpoint_sync_url: Default::default(),
            rpc_port: Default::default(),
            rpc_addr: Default::default(),
            devnet: false,
        }
    }

    #[async_trait::async_trait]
    impl InnerProvider for MockProvider {
        async fn get_block_with_txs(&self, _: BlockId) -> Result<Option<Block>> {
            Ok(self.block.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_head_info_fails() {
        let provider = test_utils::mock_provider(None);
        let config = test_utils::optimism_config();
        let head_info = HeadInfoQuery::get_head_info(&provider, &config).await;
        assert_eq!(test_utils::default_head_info(), head_info);
    }

    #[tokio::test]
    async fn test_get_head_info_empty_block() {
        let provider = test_utils::mock_provider(Some(Block::default()));
        let config = test_utils::optimism_config();
        let head_info = HeadInfoQuery::get_head_info(&provider, &config).await;
        assert_eq!(test_utils::default_head_info(), head_info);
    }

    #[tokio::test]
    async fn test_get_head_info_valid_block() {
        let provider = test_utils::mock_provider(test_utils::valid_block());
        let config = test_utils::optimism_config();
        let head_info = HeadInfoQuery::get_head_info(&provider, &config).await;
        assert_eq!(test_utils::default_head_info(), head_info);
    }
}
