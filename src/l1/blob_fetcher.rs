use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use bytes::Bytes;
use ethers::types::{Block, Transaction};
use eyre::Result;
use serde::Deserialize;
use serde_json::Value;

use super::decode_blob_data;
use crate::config::Config;

const BLOB_CARRYING_TRANSACTION_TYPE: u64 = 3;

/// The data contained in a batcher transaction.
/// The actual source of this data can be either calldata or blobs.
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

/// A beacon chain blob sidecar object.
/// KZG commitment and proof fields are not used in the current implementation.
#[derive(Debug, Deserialize)]
pub struct BlobSidecar {
    /// Blob index (transactions can have more than one blob)
    #[serde(deserialize_with = "deserialize_string_to_u64")]
    pub index: u64,
    /// Blob data (not decoded)
    #[serde(deserialize_with = "deserialize_blob_bytes")]
    pub blob: Vec<u8>,
}

impl BlobFetcher {
    /// Create a new blob fetcher with the given config.
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
            let tx_blob_hashes: Vec<String> = tx
                .other
                .get_deserialized("blobVersionedHashes")
                .unwrap_or(Ok(Vec::new()))
                .unwrap_or_default();

            if !self.is_valid_batcher_transaction(tx) {
                blob_index += tx_blob_hashes.len();
                continue;
            }

            let tx_type = tx.transaction_type.map(|t| t.as_u64()).unwrap_or(0);
            if tx_type != BLOB_CARRYING_TRANSACTION_TYPE {
                // this is necessary because ethers-rs wraps `bytes::Bytes` into its own type
                // that doesn't come with free conversion back to `bytes::Bytes`.
                let calldata = Bytes::from(tx.input.to_vec().clone());
                batcher_transactions_data.push(calldata);
                continue;
            }

            for blob_hash in tx_blob_hashes {
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
        tracing::debug!("fetched {} blobs for slot {}", blobs.len(), slot);

        for (blob_index, _) in indexed_blobs {
            let Some(blob_sidecar) = blobs.iter().find(|b| b.index == blob_index as u64) else {
                // This can happen in the case the blob retention window has expired
                // and the data is no longer available. This case is not handled yet.
                eyre::bail!("blob index {} not found in fetched sidecars", blob_index);
            };

            // decode the full blob
            let decoded_blob_data = decode_blob_data(&blob_sidecar.blob)?;

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

        let genesis_time = res.as_str().ok_or(eyre::eyre!("Expected string"))?;
        let genesis_time = genesis_time.parse::<u64>()?;

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

fn deserialize_blob_bytes<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    let s = s.trim_start_matches("0x");
    let bytes = hex::decode(s).map_err(serde::de::Error::custom)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use ethers::providers::{Http, Middleware, Provider};

    use super::*;
    use crate::config::ChainConfig;

    // TODO: update with a test from mainnet after dencun is active
    #[tokio::test]
    async fn test_get_blobs() {
        let Ok(l1_beacon_url) = std::env::var("L1_GOERLI_BEACON_RPC_URL") else {
            println!("L1_GOERLI_BEACON_RPC_URL not set; skipping test");
            return;
        };

        let config = Arc::new(Config {
            l1_beacon_url,
            ..Default::default()
        });

        let slot_number = 7576509;
        let fetcher = BlobFetcher::new(config);
        let blobs = fetcher.fetch_blob_sidecars(slot_number).await.unwrap();

        assert_eq!(blobs.len(), 6);
    }

    // TODO: update with a test from mainnet after dencun is active
    // also, this test will be flaky as nodes start to purge old blobs
    #[tokio::test]
    async fn test_get_batcher_transactions() {
        let Ok(l1_beacon_url) = std::env::var("L1_GOERLI_BEACON_RPC_URL") else {
            println!("L1_GOERLI_BEACON_RPC_URL not set; skipping test");
            return;
        };
        let Ok(l1_rpc_url) = std::env::var("L1_TEST_RPC_URL") else {
            println!("L1_TEST_RPC_URL not set; skipping test");
            return;
        };

        let config = Arc::new(Config {
            l1_beacon_url,
            chain: ChainConfig::optimism_goerli(),
            ..Default::default()
        });

        let l1_block_number = 10515928;
        let l1_provider = Provider::<Http>::try_from(l1_rpc_url).unwrap();
        let l1_block = l1_provider
            .get_block_with_txs(l1_block_number)
            .await
            .unwrap()
            .unwrap();

        let fetcher = BlobFetcher::new(config);
        let batcher_transactions = fetcher.get_batcher_transactions(&l1_block).await.unwrap();

        assert_eq!(batcher_transactions.len(), 1);
    }
}
