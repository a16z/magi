use std::{fmt::Display, net::SocketAddr, sync::Arc};

use crate::config::Config;

use eyre::Result;

use ethers::{
    providers::{Middleware, Provider},
    types::{Block, BlockId, H256},
    utils::keccak256,
};

use jsonrpsee::{
    core::{async_trait, Error},
    proc_macros::rpc,
    server::ServerBuilder,
};

use serde::{Deserialize, Serialize};

#[rpc(server, namespace = "optimism")]
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
        let l2_provider = convert_err(Provider::try_from(self.config.l2_rpc_url.clone()))?;

        let block = convert_err(l2_provider.get_block(block_number).await)?
            .ok_or(Error::Custom("unable to get block".to_string()))?;
        let state_root = block.state_root;
        let block_hash = block
            .hash
            .ok_or(Error::Custom("block hash not found".to_string()))?;
        let locations = vec![];
        let block_id = Some(BlockId::from(block_hash));

        let state_proof = convert_err(
            l2_provider
                .get_proof(
                    self.config.chain.l2_to_l1_message_passer,
                    locations,
                    block_id,
                )
                .await,
        )?;

        let withdrawal_storage_root = state_proof.storage_hash;

        let output_root = compute_l2_output_root(block, state_proof.storage_hash);

        let version: H256 = Default::default();

        Ok(OutputRootResponse {
            output_root,
            version,
            state_root,
            withdrawal_storage_root,
        })
    }
}

fn convert_err<T, E: Display>(res: Result<T, E>) -> Result<T, Error> {
    res.map_err(|err| Error::Custom(err.to_string()))
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
    pub state_root: H256,
    pub withdrawal_storage_root: H256,
}
