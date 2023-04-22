use std::sync::Arc;

use crate::config::Config;

use ethers::types::{Block, BlockId, H256};

use eyre::Result;

use ethers::providers::{Middleware, Provider};
use ethers::utils::keccak256;

use jsonrpsee::{
    core::{async_trait, Error},
    proc_macros::rpc,
    server::ServerBuilder,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[rpc(server, client, namespace = "optimism")]
pub trait Rpc {
    #[method(name = "outputAtBlock")]
    async fn output_at_block(&self, block_number: u64) -> Result<OutputRootResponse, Error>;
}

#[derive(Debug)]
pub struct RpcServerImpl {
    config: Arc<Config>,
}

#[async_trait]
impl RpcServer for RpcServerImpl {
    async fn output_at_block(&self, block_number: u64) -> Result<OutputRootResponse, Error> {
        let l2_provider =
            Provider::try_from(self.config.l2_rpc_url.clone()).expect("invalid L2 RPC url");

        let block = l2_provider.get_block(block_number).await.unwrap().unwrap();

        let state_root = block.state_root;
        let block_hash = block.hash.unwrap();
        let locations = vec![];
        let block_id = Some(BlockId::from(block_hash));

        let state_proof = l2_provider
            .get_proof(self.config.chain.l2_to_l1_message_parser_address, locations, block_id)
            .await
            .unwrap();

        let output_root = compute_l2_output_root(block, state_proof.storage_hash);
        // TODO: Verify proof like this - https://github.com/ethereum-optimism/optimism/blob/b65152ca11d7d4c2f23156af8a03339b6798c04d/op-node/node/api.go#L111

        let version: H256 = Default::default();

        Ok(OutputRootResponse {
            output_root,
            version,
        })
    }
}

fn compute_l2_output_root(block: Block<H256>, storage_root: H256) -> H256 {
    let version: H256 = Default::default();
    let digest = keccak256(
        [
            version.to_fixed_bytes(),
            block.state_root.to_fixed_bytes(),
            storage_root.to_fixed_bytes(),
            block.hash.unwrap().to_fixed_bytes(),
        ]
        .concat(),
    );

    H256::from_slice(&digest)
}

pub async fn run_server(config: Arc<Config>) -> Result<SocketAddr> {
    let server = ServerBuilder::default().build("127.0.0.1:9545").await?;
    let addr = server.local_addr()?;
    let rpc_impl = RpcServerImpl { config };
    let handle = server.start(rpc_impl.into_rpc())?;

    // In this example we don't care about doing shutdown so let's it run forever.
    // You may use the `ServerHandle` to shut it down or manage it yourself.
    tokio::spawn(handle.stopped());
    tracing::info!("rpc server started at port 9545");

    Ok(addr)
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OutputRootResponse {
    pub output_root: H256,
    pub version: H256,
}
