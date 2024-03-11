use std::sync::atomic::{AtomicU64, Ordering};

use eyre::Result;
use serde::Deserialize;
use serde_json::Value;

/// The blob fetcher is responsible for fetching blob data from the L1 beacon chain,
/// along with relevant parsing and validation.
///
/// Consensus layer info required for deriving the slot at which a specific blob was
/// included in the beacon chain is fetched on the first call to [`Self::get_slot_from_time`]
/// and cached for all subsequent calls.
pub struct BlobFetcher {
    l1_beacon_url: String,
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
    pub fn new(l1_beacon_url: String) -> Self {
        Self {
            l1_beacon_url,
            client: reqwest::Client::new(),
            genesis_timestamp: AtomicU64::new(0),
            seconds_per_slot: AtomicU64::new(0),
        }
    }

    /// Given a timestamp, return the slot number at which the timestamp
    /// was included in the beacon chain.
    ///
    /// This method uses a cached genesis timestamp and seconds per slot
    /// value to calculate the slot number. If the cache is empty, it fetches
    /// the required data from the beacon RPC.
    pub async fn get_slot_from_time(&self, time: u64) -> Result<u64> {
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

    /// Fetch the blob sidecars for a given slot.
    pub async fn fetch_blob_sidecars(&self, slot: u64) -> Result<Vec<BlobSidecar>> {
        let base_url = format!("{}/eth/v1/beacon/blob_sidecars", self.l1_beacon_url);
        let full_url = format!("{}/{}", base_url, slot);

        let res = self.client.get(full_url).send().await?.error_for_status()?;
        let res = serde_json::from_slice::<Value>(&res.bytes().await?)?;
        let res = res.get("data").ok_or(eyre::eyre!("No data in response"))?;

        let blobs = serde_json::from_value::<Vec<BlobSidecar>>(res.clone())?;

        Ok(blobs)
    }

    /// Fetch the genesis timestamp from the beacon chain.
    pub async fn fetch_beacon_genesis_timestamp(&self) -> Result<u64> {
        let base_url = format!("{}/eth/v1/beacon/genesis", self.l1_beacon_url);

        let res = self.client.get(base_url).send().await?.error_for_status()?;
        let res = serde_json::from_slice::<Value>(&res.bytes().await?)?;
        let res = res.get("data").ok_or(eyre::eyre!("No data in response"))?;
        let res = res.get("genesis_time").ok_or(eyre::eyre!("No time"))?;

        let genesis_time = res.as_str().ok_or(eyre::eyre!("Expected string"))?;
        let genesis_time = genesis_time.parse::<u64>()?;

        Ok(genesis_time)
    }

    /// Fetch the beacon chain spec.
    pub async fn fetch_beacon_spec(&self) -> Result<Value> {
        let base_url = format!("{}/eth/v1/config/spec", self.l1_beacon_url);

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

    use super::*;

    // TODO: update with a test from mainnet after dencun is active
    #[tokio::test]
    async fn test_get_blobs() {
        let Ok(l1_beacon_url) = std::env::var("L1_GOERLI_BEACON_RPC_URL") else {
            println!("L1_GOERLI_BEACON_RPC_URL not set; skipping test");
            return;
        };

        let slot_number = 7576509;
        let fetcher = BlobFetcher::new(l1_beacon_url);
        let blobs = fetcher.fetch_blob_sidecars(slot_number).await.unwrap();

        assert_eq!(blobs.len(), 6);
    }
}
