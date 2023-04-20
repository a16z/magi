use std::sync::Arc;

use crate::config::Config;

use ethers::types::H256;

use eyre::Result;

use ethers::providers::{Middleware, Provider};

use jsonrpsee::{
    core::{async_trait, Error},
    proc_macros::rpc,
    server::ServerBuilder,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[rpc(server, client, namespace = "rpc")]
pub trait Rpc {
    #[method(name = "optimism_outputAtBlock")]
    async fn optimism_output_at_block(&self, block_number: u64) -> Result<H256, Error>;
}

#[derive(Debug)]
pub struct RpcServerImpl {
    config: Arc<Config>,
}

#[async_trait]
impl RpcServer for RpcServerImpl {
    async fn optimism_output_at_block(&self, block_number: u64) -> Result<H256, Error> {
        let l2_provider =
            Provider::try_from(self.config.l2_rpc_url.clone()).expect("invalid L2 RPC url");
        let block = l2_provider.get_block(block_number).await.unwrap().unwrap();

        // let output_response: OutputRootResponse = serde_json::from_value(response)?;

        Ok(block.state_root)
    }
}

pub async fn run_server(config: Arc<Config>) -> Result<SocketAddr> {
    let server = ServerBuilder::default().build("127.0.0.1:9545").await?;
    let addr = server.local_addr()?;
    let rpc_impl = RpcServerImpl { config };
    let handle = server.start(rpc_impl.into_rpc())?;

    // In this example we don't care about doing shutdown so let's it run forever.
    // You may use the `ServerHandle` to shut it down or manage it yourself.
    tokio::spawn(handle.stopped());

    Ok(addr)
}

#[derive(Serialize, Deserialize)]
pub struct OutputRootResponse {
    pub l2_root_output: H256,
    pub version: H256,
}
