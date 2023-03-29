use std::str::FromStr;

use ethers_providers::{Provider, Http, Middleware, RetryClient, HttpRateLimitRetryPolicy};
use eyre::Result;

use crate::{engine::PayloadAttributes, common::RawTransaction};

pub struct UnsafeWatcher {
    provider: Provider<RetryClient<Http>>,
}

impl UnsafeWatcher {
    pub fn new(unsafe_l2_url: &str) -> Result<Self> {
        let http = Http::from_str(&unsafe_l2_url).map_err(|_| eyre::eyre!("invalid RPC URL"))?;
        let policy = Box::new(HttpRateLimitRetryPolicy);
        let client = RetryClient::new(http, policy, 100, 50);
        let provider = Provider::new(client);

        Ok(Self { provider })
    }

    pub async fn get_attributes(&self, start_block: u64) -> Vec<PayloadAttributes> {
        let mut attributes = Vec::new();
        let mut num = start_block;

        while let Some(a) = self.attributes_at(num).await {
            attributes.push(a);
            num += 1;
        }

        attributes
    }

    async fn attributes_at(&self, num: u64) -> Option<PayloadAttributes> {
        let block = self.provider.get_block_with_txs(num).await.unwrap()?;
        let transactions = block.transactions.iter().map(|tx| RawTransaction(tx.rlp().to_vec())).collect();

         Some(PayloadAttributes {
             timestamp: block.timestamp.as_u64().into(),
             prev_randao: block.mix_hash?,
             suggested_fee_recipient: block.author?,
             gas_limit: block.gas_limit.as_u64().into(),
             transactions: Some(transactions),
             no_tx_pool: true,
             l1_origin: None,
             seq_number: None,
             epoch: None,
             parent_hash: Some(block.parent_hash),
         })
    }
}

