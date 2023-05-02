use std::collections::HashMap;
use std::time::SystemTime;

use eyre::Result;
use reqwest::{header, Client};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::engine::DEFAULT_AUTH_PORT;
use crate::engine::ENGINE_GET_PAYLOAD_V1;

use super::{
    Engine, ExecutionPayload, ForkChoiceUpdate, ForkchoiceState, JwtSecret, PayloadAttributes,
    PayloadId, PayloadStatus, ENGINE_FORKCHOICE_UPDATED_V1, ENGINE_NEW_PAYLOAD_V1,
};

use super::{JSONRPC_VERSION, STATIC_ID};

/// An external op-geth engine api client
#[derive(Debug, Clone)]
pub struct EngineApi {
    /// Base request url
    pub base_url: String,
    /// The url port
    pub port: u16,
    /// HTTP Client
    pub client: Option<Client>,
    /// A [crate::engine::JwtSecret] used to authenticate with the engine api
    secret: JwtSecret,
}

impl EngineApi {
    /// Creates a new [`EngineApi`] with a base url and secret.
    pub fn new(base_url: &str, secret_str: &str) -> Self {
        let secret = JwtSecret::from_hex(secret_str).unwrap();

        // Gracefully parse the port from the base url
        let parts: Vec<&str> = base_url.split(':').collect();
        let port = parts[parts.len() - 1]
            .parse::<u16>()
            .unwrap_or(DEFAULT_AUTH_PORT);
        let base_url = if parts.len() <= 2 {
            parts[0].to_string()
        } else {
            parts.join(":")
        };

        let client = reqwest::Client::builder()
            .default_headers({
                header::HeaderMap::from_iter([(
                    header::CONTENT_TYPE,
                    header::HeaderValue::from_static("application/json"),
                )])
            })
            .build()
            .expect("reqwest::Client could not be built, TLS backend could not be initialized");

        Self {
            base_url,
            port,
            client: Some(client),
            secret,
        }
    }

    /// Constructs the base engine api url for the given address
    pub fn auth_url_from_addr(addr: &str, port: Option<u16>) -> String {
        let stripped = addr.strip_prefix("http://").unwrap_or(addr);
        let stripped = addr.strip_prefix("https://").unwrap_or(stripped);
        let port = port.unwrap_or(DEFAULT_AUTH_PORT);
        format!("http://{stripped}:{port}")
    }

    /// Returns if the provided secret matches the secret used to authenticate with the engine api.
    pub fn check_secret(&self, secret: &str) -> bool {
        self.secret.equal(secret)
    }

    /// Creates an engine api from environment variables
    pub fn from_env() -> Self {
        let base_url = std::env::var("ENGINE_API_URL").unwrap_or_else(|_| {
            panic!(
                "ENGINE_API_URL environment variable not set. \
                Please set this to the base url of the engine api"
            )
        });
        let secret_key = std::env::var("JWT_SECRET").unwrap_or_else(|_| {
            panic!(
                "JWT_SECRET environment variable not set. \
                Please set this to the 256 bit hex-encoded secret key used to authenticate with the engine api. \
                This should be the same as set in the `--auth.secret` flag when executing go-ethereum."
            )
        });
        let base_url = EngineApi::auth_url_from_addr(&base_url, None);
        Self::new(&base_url, &secret_key)
    }

    /// Construct base body
    pub fn base_body(&self) -> HashMap<String, Value> {
        let mut map = HashMap::new();
        map.insert(
            "jsonrpc".to_string(),
            Value::String(JSONRPC_VERSION.to_string()),
        );
        map.insert("id".to_string(), Value::Number(STATIC_ID.into()));
        map
    }

    /// Helper to construct a post request through the client
    async fn post<P>(&self, method: &str, params: Vec<Value>) -> Result<P>
    where
        P: DeserializeOwned,
    {
        // Construct the request params
        let mut body = self.base_body();
        body.insert("method".to_string(), Value::String(method.to_string()));
        body.insert("params".to_string(), Value::Array(params));

        tracing::debug!("Sending request to url: {:?}", self.base_url);
        tracing::debug!("Sending request: {:?}", serde_json::to_string(&body));

        // Send the client request
        let client = self
            .client
            .as_ref()
            .ok_or(eyre::eyre!("Driver missing http client"))?;

        // Construct the JWT Authorization Token
        let claims = self.secret.generate_claims(Some(SystemTime::now()));
        let jwt = self
            .secret
            .encode(&claims)
            .map_err(|_| eyre::eyre!("EngineApi failed to encode jwt with claims!"))?;

        // Send the request
        let res = client
            .post(&self.base_url)
            .header(header::AUTHORIZATION, format!("Bearer {}", jwt))
            .json(&body)
            .send()
            .await
            .map_err(|e| eyre::eyre!(e))?
            .json::<EngineApiResponse<P>>()
            .await?;

        if let Some(res) = res.result {
            return Ok(res);
        }

        if let Some(err) = res.error {
            eyre::bail!(err.message);
        }

        // This scenario shouldn't occur as the response should always have either data or an error
        eyre::bail!("Failed to parse Engine API response")
    }

    /// Calls the engine to verify it's available to receive requests
    pub async fn is_available(&self) -> bool {
        self.post::<Value>("eth_chainId", vec![]).await.is_ok()
    }
}

/// Generic Engine API response
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineApiResponse<P> {
    /// JSON RPC version
    jsonrpc: String,
    /// Request ID
    id: u64,
    /// JSON RPC payload
    result: Option<P>,
    /// JSON RPC error payload
    error: Option<EngineApiErrorPayload>,
}

/// Engine API error payload
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineApiErrorPayload {
    /// The error code
    pub code: i64,
    /// The error message
    pub message: String,
    /// Optional additional error data
    pub data: Option<Value>,
}

#[async_trait::async_trait]
impl Engine for EngineApi {
    async fn forkchoice_updated(
        &self,
        forkchoice_state: ForkchoiceState,
        payload_attributes: Option<PayloadAttributes>,
    ) -> Result<ForkChoiceUpdate> {
        let payload_attributes_param = match payload_attributes {
            Some(payload_attributes) => serde_json::to_value(payload_attributes)?,
            None => Value::Null,
        };
        let forkchoice_state_param = serde_json::to_value(forkchoice_state)?;
        let params = vec![forkchoice_state_param, payload_attributes_param];
        let res = self.post(ENGINE_FORKCHOICE_UPDATED_V1, params).await?;
        Ok(res)
    }

    async fn new_payload(&self, execution_payload: ExecutionPayload) -> Result<PayloadStatus> {
        let params = vec![serde_json::to_value(execution_payload)?];
        let res = self.post(ENGINE_NEW_PAYLOAD_V1, params).await?;
        Ok(res)
    }

    async fn get_payload(&self, payload_id: PayloadId) -> Result<ExecutionPayload> {
        let encoded = format!("{:x}", payload_id);
        let padded = format!("0x{:0>16}", encoded);
        let params = vec![Value::String(padded)];
        let res = self.post(ENGINE_GET_PAYLOAD_V1, params).await?;
        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    // use std::str::FromStr;
    // use ethers_core::types::H256;

    use super::*;

    const AUTH_ADDR: &str = "0.0.0.0";
    const SECRET: &str = "f79ae8046bc11c9927afe911db7143c51a806c4a537cc08e0d37140b0192f430";

    #[tokio::test]
    async fn test_engine_get_payload() {
        // Construct the engine api client
        let base_url = EngineApi::auth_url_from_addr(AUTH_ADDR, Some(8551));
        assert_eq!(base_url, "http://0.0.0.0:8551");
        let engine_api = EngineApi::new(&base_url, SECRET);
        assert_eq!(engine_api.base_url, "http://0.0.0.0:8551");
        assert_eq!(engine_api.port, 8551);

        // Construct mock server params
        let secret = JwtSecret::from_hex(SECRET).unwrap();
        let claims = secret.generate_claims(Some(SystemTime::UNIX_EPOCH));
        let jwt = secret.encode(&claims).unwrap();
        assert_eq!(jwt, String::from("eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJpYXQiOjAsImV4cCI6NjB9.rJv_krfkQefjWnZxrpnDimR1NN1UEUffK3hQzD1KInA"));
        // let bearer = format!("Bearer {jwt}");
        // let expected_body = r#"{"jsonrpc": "2.0", "method": "engine_getPayloadV1", "params": [""], "id": 1}"#;
        // let mock_response = ExecutionPayloadResponse {
        //     jsonrpc: "2.0".to_string(),
        //     id: 1,
        //     result: ExecutionPayload {
        //         parent_hash: H256::from(
        //     }
        // };

        // Create the mock server
        // let server = ServerBuilder::default()
        //     .set_id_provider(RandomStringIdProvider::new(16))
        //     .set_middleware(middleware)
        //     .build(addr.parse::<SocketAddr>().unwrap())
        //     .await
        //     .unwrap();

        // Query the engine api client
        // let execution_payload = engine_api.get_payload(PayloadId::default()).await.unwrap();
        // let expected_block_hash =
        //     H256::from_str("0xdc0818cf78f21a8e70579cb46a43643f78291264dda342ae31049421c82d21ae")
        //         .unwrap();
        // assert_eq!(expected_block_hash, execution_payload.block_hash);

        // Stop the server
        // server.stop().unwrap();
        // server.stopped().await;
    }
}
