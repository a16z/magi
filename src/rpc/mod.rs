use std::{fmt::Display, net::SocketAddr, str::FromStr, sync::Arc};

use crate::{
    config::{ChainConfig, Config},
    types::rpc::SyncStatus,
};
use crate::{types::common::HeadInfo, version::Version};

use arc_swap::ArcSwap;
use eyre::Result;

use ethers::{
    providers::{Middleware, Provider},
    types::{Address, Block, BlockId, H256},
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
    async fn output_at_block(&self, block_num_str: String) -> Result<OutputRootResponse, Error>;

    #[method(name = "syncStatus")]
    async fn sync_status(&self) -> Result<SyncStatus, Error>;

    #[method(name = "rollupConfig")]
    async fn rollup_config(&self) -> Result<Arc<ChainConfig>, Error>;

    #[method(name = "version")]
    async fn version(&self) -> Result<String, Error>;
}

#[derive(Debug)]
pub struct RpcServerImpl {
    sync_status: Arc<ArcSwap<SyncStatus>>,
    version: Version,
    config: Arc<Config>,
}

#[async_trait]
impl RpcServer for RpcServerImpl {
    async fn output_at_block(&self, block_num_str: String) -> Result<OutputRootResponse, Error> {
        let block_number = u64::from_str_radix(block_num_str.trim_start_matches("0x"), 16)
            .map_err(|_| Error::Custom("unable to parse block number".to_string()))?;

        let l2_provider = convert_err(Provider::try_from(self.config.l2_rpc_url.clone()))?;

        let block = convert_err(l2_provider.get_block_with_txs(block_number).await)?
            .ok_or(Error::Custom("unable to get block".to_string()))?;

        let state_root = block.state_root;

        let block_hash = block
            .hash
            .ok_or(Error::Custom("block hash not found".to_string()))?;

        let block_ref = HeadInfo::try_from(block.clone())
            .map_err(|_| Error::Custom("unable to parse block into head info".to_string()))?;

        let message_parser =
            Address::from_str("0x4200000000000000000000000000000000000016").unwrap();
        let locations = vec![];
        let block_id = Some(BlockId::from(block_hash));
        let state_proof = convert_err(
            l2_provider
                .get_proof(message_parser, locations, block_id)
                .await,
        )?;

        let withdrawal_storage_root = state_proof.storage_hash;

        let output_root = compute_l2_output_root(block.into(), state_proof.storage_hash);

        let version: H256 = Default::default();

        let sync_status = (*self.sync_status.load()).clone();

        Ok(OutputRootResponse {
            output_root,
            version,
            block_ref,
            state_root,
            withdrawal_storage_root,
            sync_status: *sync_status,
        })
    }

    async fn sync_status(&self) -> Result<SyncStatus, Error> {
        let sync_status = (*self.sync_status.load()).clone();
        Ok(*sync_status)
    }

    async fn rollup_config(&self) -> Result<Arc<ChainConfig>, Error> {
        Ok(Arc::clone(&self.config.chain))
    }

    async fn version(&self) -> Result<String, Error> {
        Ok(self.version.to_string())
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

pub async fn run_server(
    config: Arc<Config>,
    sync_status: Arc<ArcSwap<SyncStatus>>,
) -> Result<SocketAddr> {
    let port = config.rpc_port;
    let server = ServerBuilder::default()
        .build(format!("0.0.0.0:{}", port))
        .await?;
    let addr = server.local_addr()?;

    let rpc_impl = RpcServerImpl {
        config,
        version: Version::build(),
        sync_status,
    };
    let handle = server.start(rpc_impl.into_rpc())?;

    // In this example we don't care about doing shutdown so let's it run forever.
    // You may use the `ServerHandle` to shut it down or manage it yourself.
    tokio::spawn(handle.stopped());
    tracing::info!("rpc server started at port {}", port);

    Ok(addr)
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OutputRootResponse {
    pub version: H256,
    pub output_root: H256,
    pub block_ref: HeadInfo,
    pub withdrawal_storage_root: H256,
    pub state_root: H256,
    pub sync_status: SyncStatus,
}
