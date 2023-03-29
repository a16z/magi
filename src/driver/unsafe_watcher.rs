use ethers_providers::{Provider, Http};
use ethers_core::{utils::serialize, types::{H256, U64, H160}};
use serde::{Deserialize, Serialize};
use eyre::Result;

use crate::{engine::PayloadAttributes, common::RawTransaction};

pub struct UnsafeWatcher {
    provider: Provider<Http>,
}

impl UnsafeWatcher {
    pub fn new(sequencer_url: &str) -> Result<Self> {
        let provider = Provider::try_from(sequencer_url.clone())?;
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
        let block: Option<Block> = self.provider.request("eth_getBlockByNumber", [serialize(&num), serialize(&true)]).await.ok()?;

        block.map(|block| {
            PayloadAttributes {
                timestamp: block.timestamp,
                prev_randao: block.prev_randao,
                suggested_fee_recipient: block.suggested_fee_recipient,
                gas_limit: block.gas_limit,
                transactions: Some(block.transactions),
                no_tx_pool: true,
                l1_origin: None,
                seq_number: None,
                epoch: None,
                parent_hash: Some(block.parent_hash),
            }
        })
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Block {
    parent_hash: H256,
    timestamp: U64,
    prev_randao: H256,
    suggested_fee_recipient: H160,
    gas_limit: U64,
    transactions: Vec<RawTransaction>,
}
