use std::sync::Arc;

use crate::config::Config;

use ethers::types::H256;

use eyre::Result;

use jsonrpsee::{
    core::client::ClientT,
    http_client::{transport::HttpBackend, HttpClientBuilder},
    rpc_params,
};
use serde::{Deserialize, Serialize};

pub struct Rpc {
    l2_rpc: jsonrpsee::http_client::HttpClient<HttpBackend>,
}

#[derive(Serialize, Deserialize)]
pub struct OutputRootResponse {
    pub l2_root_output: H256,
    pub version: H256,
}

impl Rpc {
    pub fn new(config: &Arc<Config>) -> Result<Self> {
        let l2_rpc = HttpClientBuilder::default()
            .build(config.l2_rpc_url.clone())
            .unwrap();

        Ok(Self { l2_rpc })
    }

    pub async fn get_output_root_at_block(&self, block_number: u64) -> Result<H256> {
        let params = rpc_params![block_number];
        let response = self
            .l2_rpc
            .request("optimism_outputAtBlock", params)
            .await?;
        let output_response: OutputRootResponse = serde_json::from_value(response)?;

        Ok(output_response.l2_root_output)
    }
}
