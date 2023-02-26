use std::collections::HashMap;

use eyre::Result;
use reqwest::Client;

use super::{L2EngineApi, PayloadId, ExecutionPayload, ForkchoiceState, PayloadAttributes, ForkChoiceUpdate, PayloadStatus};

/// An external op-geth engine driver
#[derive(Debug, Clone)]
pub struct ExternalDriver {
    /// Base request url
    pub base_url: String,
    /// HTTP Client
    pub client: Option<Client>,
}

impl ExternalDriver {
    /// Creates a new external driver
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: None
        }
    }

    /// Construct base body
    pub fn base_body(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        map.insert("jsonrpc".to_string(), "2.0".to_string());
        map.insert("id".to_string(), "1".to_string());
        map
    }

    /// Helper to construct a post request through the client
    pub async fn post(&self, endpoint: &str, mut body: HashMap<String, String>) -> Result<reqwest::Response> {
        // Construct the request params
        let url = format!("{}/{}", self.base_url, endpoint);
        let base_body = self.base_body();
        let _ = base_body.into_iter().map(|(k, v)| body.insert(k, v).ok_or(eyre::eyre!("Failed to insert key")));

        // Send the client request
        let client = self.client.as_ref().ok_or(eyre::eyre!("Driver missing http client"))?;
        client.post(url)
            .json(&body)
            .send()
            .await
            .map_err(|e| eyre::eyre!(e))
    }
}

impl L2EngineApi for ExternalDriver {
    fn forkchoice_updated(forkchoiceState: ForkchoiceState, payloadAttributes: Option<PayloadAttributes>) -> Result<ForkChoiceUpdate> {

        Err(eyre::eyre!("Not implemented"))
    }

    fn new_payload(executionPayload: ExecutionPayload) -> Result<PayloadStatus> {

        Err(eyre::eyre!("Not implemented"))
    }

    fn get_payload(payloadId: PayloadId) -> Result<ExecutionPayload> {

        Err(eyre::eyre!("Not implemented"))
    }
}