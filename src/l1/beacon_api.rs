use eyre::Result;
use serde_json::Value;

pub struct L1BeaconApi {
    l1_beacon_url: String,
    client: reqwest::Client,
}

pub enum BlobFilter {
    Slot(u64),
    BlockRoot(String),
    Head,
    Genesis,
    Finalized,
}

impl L1BeaconApi {
    pub fn new(l1_beacon_url: String) -> Self {
        Self {
            l1_beacon_url,
            client: reqwest::Client::new(),
        }
    }

    /// Get all blobs for a specific slot
    pub async fn get_blob_sidecars(&self, filter: BlobFilter) -> Result<Value> {
        let base_url = format!("{}/eth/v1/beacon/blob_sidecars", self.l1_beacon_url);
        let full_url = match filter {
            BlobFilter::Head => format!("{}/head", base_url),
            BlobFilter::Genesis => format!("{}/genesis", base_url),
            BlobFilter::Finalized => format!("{}/finalized", base_url),
            BlobFilter::BlockRoot(block_root) => format!("{}/{}", base_url, block_root),
            BlobFilter::Slot(slot) => format!("{}/{}", base_url, slot),
        };

        let resp = self.client.get(full_url).send().await?.error_for_status()?;

        let body = resp.text().await?;
        let blob = serde_json::from_str(&body)?;
        Ok(blob)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_blobs() {
        // TODO: use env vars in tests
        // let Ok(l1_beacon_url) = std::env::var("L1_BEACON_TEST_RPC_URL") else {
        //     return;
        // };
        let l1_beacon_url = "https://remotelab.taila355b.ts.net".to_string();

        let retriever = L1BeaconApi::new(l1_beacon_url);
        let blobs = retriever.get_blob_sidecars(BlobFilter::Head).await.unwrap();

        println!("blobs: {:?}", blobs);
    }
}
