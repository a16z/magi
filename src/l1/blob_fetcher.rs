use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use bytes::Bytes;
use ethers::types::{Block, Transaction, H256};
use eyre::Result;
use serde::Deserialize;
use serde_json::Value;

use super::decode_blob_data;
use crate::config::Config;

const BLOB_CARRYING_TRANSACTION_TYPE: u64 = 3;

pub type BatcherTransactionData = Bytes;

/// The blob fetcher is responsible for fetching blob data from the L1 beacon chain,
/// along with relevant parsing and validation.
///
/// Consensus layer info required for deriving the slot at which a specific blob was
/// included in the beacon chain is fetched on the first call to [`Self::get_slot_from_time`]
/// and cached for all subsequent calls.
pub struct BlobFetcher {
    config: Arc<Config>,
    client: reqwest::Client,
    genesis_timestamp: AtomicU64,
    seconds_per_slot: AtomicU64,
}

#[derive(Debug, Deserialize)]
pub struct BlobSidecar {
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub index: u64,
    pub blob: Bytes,
    // kzg_commitment: String,
    // kzg_proof: String,
    // signed_block_header: Value,
    // kzg_commitment_inclusion_proof: Vec<String>,
}

impl BlobFetcher {
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
            genesis_timestamp: AtomicU64::new(0),
            seconds_per_slot: AtomicU64::new(0),
        }
    }

    /// Given a block, return a list of `BatcherTransactionData` containing either the
    /// calldata or the decoded blob data for each batcher transaction in the block.
    pub async fn get_batcher_transactions(
        &self,
        block: &Block<Transaction>,
    ) -> Result<Vec<BatcherTransactionData>> {
        let mut batcher_transactions_data = Vec::new();
        let mut indexed_blobs = Vec::new();
        let mut blob_index = 0;

        for tx in block.transactions.iter() {
            let tx_blob_hashes = tx.other.get("blob_versioned_hashes");
            dbg!(tx_blob_hashes);

            if !self.is_valid_batcher_transaction(tx) {
                blob_index += 1; // TODO: += number of actual tx.blob_hashes
                continue;
            }

            // sanity check: transactions here should always have a transaction type
            let Some(tx_type) = tx.transaction_type.map(|t| t.as_u64()) else {
                tracing::error!("found batcher tx without tx_type. This shouldn't happen.");
                continue;
            };

            if tx_type != BLOB_CARRYING_TRANSACTION_TYPE {
                // this is necessary because ethers-rs wraps `bytes::Bytes` into its own type
                // that doesn't come with free conversion back to `bytes::Bytes`.
                let calldata = Bytes::from(tx.input.to_vec().clone());
                batcher_transactions_data.push(calldata);
                continue;
            }

            // TODO: retrieve tx.blob_hashes. might need to update/fork ethers-rs.
            // are there other ways to see how many blobs are in a tx?
            let blob_hashes = vec![H256::zero()];
            for blob_hash in blob_hashes {
                indexed_blobs.push((blob_index, blob_hash));
                blob_index += 1;
            }
        }

        // if at this point there are no blobs, return early
        if indexed_blobs.is_empty() {
            return Ok(batcher_transactions_data);
        }

        let slot = self.get_slot_from_time(block.timestamp.as_u64()).await?;
        // perf: fetch only the required indexes instead of all
        let blobs = self.fetch_blob_sidecars(slot).await?;

        for (blob_index, blob_hash) in indexed_blobs {
            let Some(blob_sidecar) = blobs.iter().find(|b| b.index == blob_index) else {
                // This can happen in the case the blob retention window has expired
                // and the data is no longer available. This case is not handled yet.
                eyre::bail!("blob index {} not found in fetched sidecars", blob_index);
            };

            // decode the full blob
            let decoded_blob_data = decode_blob_data(&blob_sidecar.blob)?;
            tracing::debug!("successfully decoded blob data for hash {:?}", blob_hash);

            batcher_transactions_data.push(decoded_blob_data);
        }

        Ok(batcher_transactions_data)
    }

    #[inline]
    fn is_valid_batcher_transaction(&self, tx: &Transaction) -> bool {
        let batch_sender = self.config.chain.system_config.batch_sender;
        let batch_inbox = self.config.chain.batch_inbox;

        tx.from == batch_sender && tx.to.map(|to| to == batch_inbox).unwrap_or(false)
    }

    #[inline]
    async fn get_slot_from_time(&self, time: u64) -> Result<u64> {
        let mut genesis_timestamp = self.genesis_timestamp.load(Ordering::Relaxed);
        let mut seconds_per_slot = self.seconds_per_slot.load(Ordering::Relaxed);

        // If we don't have data about the genesis timestamp, we need to fetch it
        // from the CL first along with the "SECONDS_PER_SLOT" value from the spec.
        if genesis_timestamp == 0 {
            genesis_timestamp = self.fetch_beacon_genesis_timestamp().await?;
            self.genesis_timestamp
                .store(genesis_timestamp, Ordering::Relaxed);

            let spec = self.fetch_beacon_spec().await?;
            seconds_per_slot = spec
                .get("SECONDS_PER_SLOT")
                .ok_or(eyre::eyre!("No seconds per slot in beacon spec"))?
                .as_str()
                .ok_or(eyre::eyre!("Seconds per slot: expected string"))?
                .parse::<u64>()?;

            if seconds_per_slot == 0 {
                eyre::bail!("Seconds per slot is 0; cannot calculate slot number");
            }

            self.seconds_per_slot
                .store(seconds_per_slot, Ordering::Relaxed);
        }

        if time < genesis_timestamp {
            eyre::bail!("Time is before genesis; cannot calculate slot number");
        }

        Ok((time - genesis_timestamp) / seconds_per_slot)
    }

    async fn fetch_blob_sidecars(&self, slot: u64) -> Result<Vec<BlobSidecar>> {
        let base_url = format!("{}/eth/v1/beacon/blob_sidecars", self.config.l1_beacon_url);
        let full_url = format!("{}/{}", base_url, slot);

        let res = self.client.get(full_url).send().await?.error_for_status()?;
        let res = serde_json::from_slice::<Value>(&res.bytes().await?)?;
        let res = res.get("data").ok_or(eyre::eyre!("No data in response"))?;

        let blobs = serde_json::from_value::<Vec<BlobSidecar>>(res.clone())?;

        Ok(blobs)
    }

    async fn fetch_beacon_genesis_timestamp(&self) -> Result<u64> {
        let base_url = format!("{}/eth/v1/beacon/genesis", self.config.l1_beacon_url);

        let res = self.client.get(base_url).send().await?.error_for_status()?;
        let res = serde_json::from_slice::<Value>(&res.bytes().await?)?;
        let res = res.get("data").ok_or(eyre::eyre!("No data in response"))?;
        let res = res.get("genesis_time").ok_or(eyre::eyre!("No time"))?;

        let genesis_time = serde_json::from_value::<u64>(res.clone())?;

        Ok(genesis_time)
    }

    async fn fetch_beacon_spec(&self) -> Result<Value> {
        let base_url = format!("{}/eth/v1/config/spec", self.config.l1_beacon_url);

        let res = self.client.get(base_url).send().await?.error_for_status()?;
        let res = serde_json::from_slice::<Value>(&res.bytes().await?)?;
        let res = res.get("data").ok_or(eyre::eyre!("No data in response"))?;

        Ok(res.clone())
    }
}

fn deserialize_string_to_u64<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    s.parse::<u64>().map_err(serde::de::Error::custom)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    // TODO: update with a test from mainnet after dencun is active
    async fn test_get_blobs() {
        let Ok(l1_beacon_url) = std::env::var("L1_GOERLI_BEACON_RPC_URL") else {
            return;
        };

        let config = Arc::new(Config {
            l1_beacon_url,
            ..Default::default()
        });

        let retriever = BlobFetcher::new(config);
        let blobs = retriever.fetch_blob_sidecars(7576509).await.unwrap();

        assert_eq!(blobs.len(), 3);
    }
}
