use bytes::Bytes;
use ethers::types::{Address, Block, Transaction};
use eyre::Result;
use serde::Deserialize;
use serde_json::Value;

const BLOB_CARRYING_TRANSACTION_TYPE: u64 = 3;

#[derive(Debug)]
pub enum BatcherTransactionData {
    Calldata(Vec<u8>),
    Blob(BlobSidecar),
}

pub struct BlobFetcher {
    l1_beacon_url: String,
    client: reqwest::Client,

    batch_sender: Address,
    batch_inbox: Address,
}

pub enum FetchBlobFilter {
    Slot(u64),
    BlockRoot(String),
}

#[derive(Debug, Deserialize)]
pub struct BlobSidecar {
    pub index: String,
    pub blob: Bytes,
    // kzg_commitment: String,
    // kzg_proof: String,
    // signed_block_header: Value,
    // kzg_commitment_inclusion_proof: Vec<String>,
}

impl BlobFetcher {
    pub fn new(l1_beacon_url: String, batch_inbox: Address, batch_sender: Address) -> Self {
        Self {
            l1_beacon_url,
            batch_inbox,
            batch_sender,
            client: reqwest::Client::new(),
        }
    }

    pub async fn fetch_blob_sidecars(&self, filter: FetchBlobFilter) -> Result<Vec<BlobSidecar>> {
        let base_url = format!("{}/eth/v1/beacon/blob_sidecars", self.l1_beacon_url);
        let full_url = match filter {
            FetchBlobFilter::BlockRoot(block_root) => format!("{}/{}", base_url, block_root),
            FetchBlobFilter::Slot(slot) => format!("{}/{}", base_url, slot),
        };

        let resp = self.client.get(full_url).send().await?.error_for_status()?;
        let resp = serde_json::from_slice::<Value>(&resp.bytes().await?)?;
        let resp = resp.get("data").ok_or(eyre::eyre!("No data in response"))?;

        let blobs = serde_json::from_value::<Vec<BlobSidecar>>(resp.clone())?;

        Ok(blobs)
    }

    pub async fn get_batcher_transactions(
        &self,
        block: &Block<Transaction>,
    ) -> Result<Vec<BatcherTransactionData>> {
        let mut batcher_transactions = Vec::new();
        let mut blob_index = 0;

        for tx in block.transactions.iter() {
            if !self.is_valid_batcher_transaction(tx) {
                blob_index += 1;
                continue;
            }

            // sanity check: transactions here should always have a transaction type
            let Some(tx_type) = tx.transaction_type.map(|t| t.as_u64()) else {
                tracing::error!("found batcher tx without tx_type. This shouldn't happen.");
                continue;
            };

            if tx_type != BLOB_CARRYING_TRANSACTION_TYPE {
                batcher_transactions.push(BatcherTransactionData::Calldata(tx.input.to_vec()));
                continue;
            }

            // TODO:
            // download blobs from the beacon chain here...
            // let slot = get_slot_from_block_number(block.number);
            // let blobs = self.fetch_blob_sidecars(FetchBlobFilter::Slot(slot)).await?;
        }

        Ok(vec![])
    }

    fn is_valid_batcher_transaction(&self, tx: &Transaction) -> bool {
        tx.from == self.batch_sender && tx.to.map(|to| to == self.batch_inbox).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    // TODO: update with a test from mainnet after dencun is active
    async fn test_get_blobs() {
        // TODO: use env vars in tests
        // let Ok(l1_beacon_url) = std::env::var("L1_BEACON_TEST_RPC_URL") else {
        //     return;
        // };
        let l1_beacon_url = "https://remotelab.taila355b.ts.net".to_string();
        let retriever = BlobFetcher::new(l1_beacon_url, Address::zero(), Address::zero());
        let blobs = retriever
            .fetch_blob_sidecars(FetchBlobFilter::Slot(4248703))
            .await
            .unwrap();

        assert_eq!(blobs.len(), 3);
    }
}
