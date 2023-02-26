use std::collections::HashMap;

use eyre::Result;
use reqwest::Client;

use crate::engine::ENGINE_GET_PAYLOAD_V1;

use super::{
    ExecutionPayload, ForkChoiceUpdate, ForkchoiceState, L2EngineApi, PayloadAttributes, PayloadId,
    PayloadStatus,
};

use super::{JSONRPC_VERSION, STATIC_ID};

/// An external op-geth engine api client
#[derive(Debug, Clone)]
pub struct EngineApi {
    /// Base request url
    pub base_url: String,
    /// HTTP Client
    pub client: Option<Client>,
}

impl EngineApi {
    /// Creates a new external api client
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: Some(reqwest::Client::new()),
        }
    }

    /// Creates an engine api from environment variables
    pub fn from_env() -> Self {
        let base_url = std::env::var("ENGINE_API_URL").unwrap_or_else(|_| {
            panic!(
                "ENGINE_API_URL environment variable not set. \
                Please set this to the base url of the engine api"
            )
        });
        Self::new(base_url)
    }

    // TODO: Abstract the body wrapping the inner hashmap in a struct and exposing convenience methods

    /// Construct base body
    pub fn base_body(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        map.insert("jsonrpc".to_string(), JSONRPC_VERSION.to_string());
        map.insert("id".to_string(), STATIC_ID.to_string());
        map
    }

    /// Helper to construct a post request through the client
    pub async fn post(
        &self,
        endpoint: &str,
        mut body: HashMap<String, String>,
    ) -> Result<reqwest::Response> {
        // Construct the request params
        let base_body = self.base_body();
        body.insert("method".to_string(), endpoint.to_string());
        let _ = base_body
            .into_iter()
            .map(|(k, v)| body.insert(k, v).ok_or(eyre::eyre!("Failed to insert key")));

        tracing::debug!("Sending request to url: {:?}", self.base_url);
        tracing::debug!("Sending request: {:?}", serde_json::to_string(&body));

        // Send the client request
        let client = self
            .client
            .as_ref()
            .ok_or(eyre::eyre!("Driver missing http client"))?;
        client
            .post(&self.base_url)
            .json(&body)
            .send()
            .await
            .map_err(|e| eyre::eyre!(e))
    }
}

#[async_trait::async_trait]
impl L2EngineApi for EngineApi {
    async fn forkchoice_updated(
        &self,
        _forkchoice_state: ForkchoiceState,
        _payload_attributes: Option<PayloadAttributes>,
    ) -> Result<ForkChoiceUpdate> {
        Err(eyre::eyre!("Not implemented"))
    }

    async fn new_payload(&self, _execution_payload: ExecutionPayload) -> Result<PayloadStatus> {
        Err(eyre::eyre!("Not implemented"))
    }

    async fn get_payload(&self, payload_id: PayloadId) -> Result<ExecutionPayload> {
        let encoded = format!("{:x}", payload_id);
        // let pad = 8 - encoded.len();
        let padded = format!("0x{:0>16}", encoded);
        println!("Padded payload id: {}", padded);
        let mut body = HashMap::new();
        let params = serde_json::to_string(&vec![padded])?;
        body.insert("params".to_string(), params);
        let res = self.post(ENGINE_GET_PAYLOAD_V1, body).await?;
        println!("Response: {:?}", res);

        Err(eyre::eyre!("Not implemented"))
    }
}
