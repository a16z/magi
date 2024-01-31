use crate::config::ChainConfig;
use crate::types::common::HeadInfo;
use ethers::middleware::Middleware;
use ethers::providers::{JsonRpcClient, Provider, ProviderError};
use ethers::types::{Block, BlockId, BlockNumber, Transaction};

use eyre::Result;

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

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct HeadInfoResult {
    pub finalized: HeadInfo,
    pub safe: HeadInfo,
    pub latest: HeadInfo,
}

pub struct HeadInfoQuery {}

impl HeadInfoQuery {
    pub async fn get_heads<P: InnerProvider>(p: &P, chain: &ChainConfig) -> Result<HeadInfoResult> {
        let mut heads = Vec::with_capacity(3);

        let block_queries = vec![
            p.get_block_with_txs(BlockId::Number(BlockNumber::Finalized)),
            p.get_block_with_txs(BlockId::Number(BlockNumber::Safe)),
            p.get_block_with_txs(BlockId::Number(BlockNumber::Latest)),
        ];

        let blocks_res = futures::future::join_all(block_queries).await;
        let default_head = HeadInfo {
            head: chain.l2_genesis(),
            epoch: chain.l1_start_epoch(),
            seq_number: 0,
        };

        for res in blocks_res {
            let head = match res {
                Ok(Some(block)) => HeadInfo::try_from(block).unwrap_or(default_head),
                Ok(None) => default_head,
                Err(err) => eyre::bail!("error fetching heads from the L2: {}", err),
            };
            heads.push(head);
        }

        match heads.as_slice() {
            [finalized, safe, latest] => Ok(HeadInfoResult {
                finalized: *finalized,
                safe: *safe,
                latest: *latest,
            }),
            _ => eyre::bail!("error during heads fetch, expected 3 elements"),
        }
    }
}

#[allow(dead_code)]
#[cfg(any(test, feature = "test-utils"))]
mod test_utils {
    use super::*;
    use crate::types::common::{BlockInfo, Epoch};
    use ethers::types::H256;
    use std::str::FromStr;

    pub struct MockProvider {
        pub finalized: Option<Block<Transaction>>,
        pub safe: Option<Block<Transaction>>,
        pub latest: Option<Block<Transaction>>,
    }

    pub fn mock_provider(
        finalized: Option<Block<Transaction>>,
        safe: Option<Block<Transaction>>,
        latest: Option<Block<Transaction>>,
    ) -> MockProvider {
        MockProvider {
            finalized,
            safe,
            latest,
        }
    }

    pub fn default_head_info() -> HeadInfo {
        HeadInfo {
            head: BlockInfo {
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
            epoch: Epoch {
                number: 17422590,
                hash: H256::from_str(
                    "438335a20d98863a4c0c97999eb2481921ccd28553eac6f913af7c12aec04108",
                )
                .unwrap(),
                timestamp: 1686068903,
            },
            seq_number: 0,
        }
    }

    pub fn block_with_deposit_tx() -> Block<Transaction> {
        let raw_block = r#"{
            "hash": "0x035cdb5c723356a08974ac87c351d6743c2d286be522ac389c6062f28dbbbf53",
            "parentHash": "0x02b06a8ea8c55f284cb879dd8820e311a96e4ddfdd0872f7f5a4f139b6811311",
            "sha3Uncles": "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
            "miner": "0x4200000000000000000000000000000000000011",
            "stateRoot": "0xf9b1ac6ce27e80e14f64f40ba8d474fad45f59493d6907bb06e0b4c798945891",
            "transactionsRoot": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "receiptsRoot": "0x251aabe436cc6057e357dc2d6b9a2091ebc3410ed938b818ae5451330229967b",
            "number": "0x1",
            "gasUsed": "0x3183d",
            "gasLimit": "0xf4240",
            "extraData": "0x",
            "logsBloom": "0x00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000008000000002000000000000000000000000000000000000000000000000000000000000000000000000000800400000000000000000000000000000000001000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000010000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000",
            "timestamp": "0x6568b7c0",
            "difficulty": "0x0",
            "sealFields": [],
            "uncles": [],
            "transactions": [
                {
                    "hash": "0x035cdb5c723356a08974ac87c351d6743c2d286be522ac389c6062f28dbbbf53",
                    "nonce": "0x28972",
                    "blockHash": "0xbee7192e575af30420cae0c7776304ac196077ee72b048970549e4f08e875453",
                    "blockNumber": "0x1",
                    "transactionIndex": "0x0",
                    "from": "0xDeaDDEaDDeAdDeAdDEAdDEaddeAddEAdDEAd0001",
                    "to": "0x4200000000000000000000000000000000000015",
                    "value": "0x0",
                    "gasPrice": "0x1",
                    "gas": "0xf4240",
                    "input": "0x015d8eb90000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000006568b7be000000000000000000000000000000000000000000000000000000003b9aca000a26d377814dd5ccddb2c743fab282235e1fa31546ccd8fb9bb9a8fc37d9063a00000000000000000000000000000000000000000000000000000000000000010000000000000000000000003c44cdddb6a900fa2b585dd299e03d12fa4293bc000000000000000000000000000000000000000000000000000000000000083400000000000000000000000000000000000000000000000000000000000f4240",
                    "r": "0x0000000000000000000000000000000000000000000000000000000000000000",
                    "s": "0x0000000000000000000000000000000000000000000000000000000000000000",
                    "v": "0x0"
                }
            ],
            "size": "0x363",
            "mixHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "nonce": "0x0000000000000001"
        }
        "#;

        serde_json::from_str(raw_block).unwrap()
    }

    pub fn block_no_deposit_tx() -> Block<Transaction> {
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
            "sealFields": [],
            "uncles": [],
            "transactions": [],
            "size": "0x365",
            "mixHash": "0x7aeec5550a9b0616701e49ab835af5f10eadba2a0582016f0e256c9cace0c046",
            "nonce": "0x0000000000000000",
            "baseFeePerGas": "0x32"
        }
        "#;
        serde_json::from_str(raw_block).unwrap()
    }

    #[async_trait::async_trait]
    impl InnerProvider for MockProvider {
        async fn get_block_with_txs(
            &self,
            block_id: BlockId,
        ) -> Result<Option<Block<Transaction>>, ProviderError> {
            match block_id {
                BlockId::Number(BlockNumber::Finalized) => Ok(self.finalized.clone()),
                BlockId::Number(BlockNumber::Safe) => Ok(self.safe.clone()),
                BlockId::Number(BlockNumber::Latest) => Ok(self.latest.clone()),
                _ => Err(ProviderError::CustomError("not supported".to_string())),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ethers::prelude::Http;
    use reqwest::Url;
    use std::time::Duration;

    #[tokio::test]
    async fn test_get_heads_no_block() -> Result<()> {
        let provider = test_utils::mock_provider(None, None, None);
        let chain = ChainConfig::optimism();
        let heads = HeadInfoQuery::get_heads(&provider, &chain).await?;

        assert_eq!(heads.finalized, test_utils::default_head_info());
        assert_eq!(heads.safe, test_utils::default_head_info());
        assert_eq!(heads.latest, test_utils::default_head_info());

        Ok(())
    }

    #[tokio::test]
    async fn test_get_finalized_block() -> Result<()> {
        let block = test_utils::block_with_deposit_tx();
        let finalized_head = HeadInfo::try_from(block.clone())?;

        let provider = test_utils::mock_provider(Some(block), None, None);
        let chain = ChainConfig::optimism();
        let heads = HeadInfoQuery::get_heads(&provider, &chain).await?;

        assert_eq!(heads.finalized, finalized_head);
        assert_eq!(heads.safe, test_utils::default_head_info());
        assert_eq!(heads.latest, test_utils::default_head_info());

        Ok(())
    }

    #[tokio::test]
    async fn test_get_safe_block() -> Result<()> {
        let block = test_utils::block_with_deposit_tx();
        let safe_head = HeadInfo::try_from(block.clone())?;

        let provider = test_utils::mock_provider(None, Some(block), None);
        let chain = ChainConfig::optimism();
        let heads = HeadInfoQuery::get_heads(&provider, &chain).await?;

        assert_eq!(heads.finalized, test_utils::default_head_info());
        assert_eq!(heads.safe, safe_head);
        assert_eq!(heads.latest, test_utils::default_head_info());

        Ok(())
    }

    #[tokio::test]
    async fn test_get_latest_block() -> Result<()> {
        let block = test_utils::block_with_deposit_tx();
        let latest_head = HeadInfo::try_from(block.clone())?;

        let provider = test_utils::mock_provider(None, None, Some(block));
        let chain = ChainConfig::optimism();
        let heads = HeadInfoQuery::get_heads(&provider, &chain).await?;

        assert_eq!(heads.finalized, test_utils::default_head_info());
        assert_eq!(heads.safe, test_utils::default_head_info());
        assert_eq!(heads.latest, latest_head);

        Ok(())
    }

    #[tokio::test]
    async fn test_block_missing_transaction() -> Result<()> {
        let provider = test_utils::mock_provider(
            Some(test_utils::block_no_deposit_tx()),
            Some(test_utils::block_no_deposit_tx()),
            Some(test_utils::block_no_deposit_tx()),
        );

        let chain = ChainConfig::optimism();
        let heads = HeadInfoQuery::get_heads(&provider, &chain).await?;

        assert_eq!(heads.finalized, test_utils::default_head_info());
        assert_eq!(heads.safe, test_utils::default_head_info());
        assert_eq!(heads.latest, test_utils::default_head_info());

        Ok(())
    }

    #[tokio::test]
    async fn test_get_heads_no_connection() -> Result<()> {
        let chain = ChainConfig::optimism();

        let client = reqwest::ClientBuilder::new()
            .timeout(Duration::from_secs(5))
            .build()?;

        let http = Http::new_with_client(Url::parse("http://10.255.255.1:0")?, client);
        let provider = Provider::new(http);
        let fetcher = HeadInfoFetcher::from(&provider);

        let result = HeadInfoQuery::get_heads(&fetcher, &chain).await;

        assert!(result.is_err());

        let err = result.err().unwrap();
        assert!(err
            .to_string()
            .contains("error sending request for url (http://10.255.255.1:0/)"));

        Ok(())
    }
}
