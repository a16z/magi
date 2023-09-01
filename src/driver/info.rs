use crate::config::Config;
use crate::driver::types::HeadInfo;
use ethers::middleware::Middleware;
use ethers::providers::{JsonRpcClient, Provider, ProviderError};
use ethers::types::{Block, BlockId, BlockNumber, Transaction};

#[async_trait::async_trait]
pub trait InnerProvider {
    async fn get_block_with_txs(
        &self,
        block_id: BlockId,
    ) -> Result<Option<Block<Transaction>>, ProviderError>;
}

pub struct HeadInfoFetcher<'a, P: JsonRpcClient> {
    inner: &'a Provider<P>,
}

impl<'a, P: JsonRpcClient> From<&'a Provider<P>> for HeadInfoFetcher<'a, P> {
    fn from(inner: &'a Provider<P>) -> Self {
        Self { inner }
    }
}

#[async_trait::async_trait]
impl<'a, P: JsonRpcClient> InnerProvider for HeadInfoFetcher<'a, P> {
    async fn get_block_with_txs(
        &self,
        block_id: BlockId,
    ) -> Result<Option<Block<Transaction>>, ProviderError> {
        self.inner.get_block_with_txs(block_id).await
    }
}

pub struct HeadInfoQuery {}

impl HeadInfoQuery {
    pub async fn get_head_info<P: InnerProvider>(p: &P, config: &Config) -> HeadInfo {
        p.get_block_with_txs(BlockId::Number(BlockNumber::Finalized))
            .await
            .ok()
            .flatten()
            .and_then(|block| HeadInfo::try_from(block).ok())
            .unwrap_or_else(|| {
                tracing::warn!("could not get head info. Falling back to the genesis head.");
                HeadInfo {
                    l2_block_info: config.chain.l2_genesis,
                    l1_epoch: config.chain.l1_start_epoch,
                    sequence_number: 0,
                }
            })
    }
}

#[cfg(any(test, feature = "test-utils"))]
mod test_utils {
    use super::*;

    pub struct MockProvider;

    #[async_trait::async_trait]
    impl InnerProvider for MockProvider {
        async fn get_block_with_txs(
            &self,
            _: BlockId,
        ) -> Result<Option<Block<Transaction>>, ProviderError> {
            Ok(Some(Block::default()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{BlockInfo, Epoch};
    use crate::config;
    use ethers::types::H256;
    use std::str::FromStr;

    #[tokio::test]
    async fn test_get_head_info() {
        let provider = test_utils::MockProvider {};
        let config = config::Config {
            l1_rpc_url: Default::default(),
            l2_rpc_url: Default::default(),
            l2_engine_url: Default::default(),
            chain: config::ChainConfig::optimism(),
            jwt_secret: Default::default(),
            checkpoint_sync_url: Default::default(),
            rpc_port: Default::default(),
        };
        let head_info = HeadInfoQuery::get_head_info(&provider, &config).await;
        let expected_head = HeadInfo {
            l2_block_info: BlockInfo {
                hash: H256::from_str(
                    "dbf6a80fef073de06add9b0d14026d6e5a86c85f6d102c36d3d8e9cf89c2afd3",
                )
                .unwrap(),
                number: 105235063,
                parent_hash: H256::from_str(
                    "21a168dfa5e727926063a28ba16fd5ee84c814e847c81a699c7a0ea551e4ca50",
                )
                .unwrap(),
                timestamp: 1686068903,
            },
            l1_epoch: Epoch {
                number: 17422590,
                hash: H256::from_str(
                    "438335a20d98863a4c0c97999eb2481921ccd28553eac6f913af7c12aec04108",
                )
                .unwrap(),
                timestamp: 1686068903,
            },
            sequence_number: 0,
        };
        assert_eq!(expected_head, head_info);
    }
}
