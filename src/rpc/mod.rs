use std::{fmt::Display, net::SocketAddr, sync::Arc};

use crate::{
    config::{Config, ExternalChainConfig},
    version::Version,
};

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

    #[method(name = "rollupConfig")]
    async fn rollup_config(&self) -> Result<ExternalChainConfig, Error>;

    #[method(name = "version")]
    async fn version(&self) -> Result<String, Error>;
}

#[derive(Debug)]
pub struct RpcServerImpl {
    version: Version,
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

    async fn rollup_config(&self) -> Result<ExternalChainConfig, Error> {
        let config = (*self.config).clone();

        Ok(ExternalChainConfig::from(config.chain))
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

pub async fn run_server(config: Arc<Config>) -> Result<SocketAddr> {
    let port = config.rpc_port;
    let addr = config.rpc_addr.clone();

    let server = ServerBuilder::default()
        .build(format!("{}:{}", addr, port))
        .await?;
    let addr = server.local_addr()?;
    let rpc_impl = RpcServerImpl {
        config,
        version: Version::build(),
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
    pub output_root: H256,
    pub version: H256,
    pub state_root: H256,
    pub withdrawal_storage_root: H256,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ChainConfig, CliConfig, ExternalChainConfig};
    use reqwest;
    use serde_json::json;
    use std::{path::PathBuf, str::FromStr};
    use tokio::time::{sleep, Duration};
    use tracing_subscriber;

    #[derive(Serialize, Deserialize, Debug)]
    struct RpcResponse {
        jsonrpc: String,
        result: ExternalChainConfig,
        id: u64,
    }

    #[tokio::test]
    async fn test_run_server() -> Result<()> {
        std::env::set_var("RUST_LOG", "trace");
        let cli_config = CliConfig {
            l1_rpc_url: Some("".to_string()),
            l2_rpc_url: None,
            l2_engine_url: None,
            jwt_secret: Some("".to_string()),
            checkpoint_sync_url: None,
            rpc_port: Some(8080),
            rpc_addr: Some("127.0.0.1".to_string()),
            devnet: false,
        };

        tracing_subscriber::fmt().init();

        let config_path = PathBuf::from_str("config.toml")?;
        let config = Arc::new(Config::new(
            &config_path,
            cli_config,
            ChainConfig::optimism_sepolia(),
        ));

        let addr = run_server(config.clone())
            .await
            .expect("Failed to start server");

        sleep(Duration::from_millis(100)).await;

        let client = reqwest::Client::new();

        let request_body = json!({
            "jsonrpc": "2.0",
            "method": "optimism_rollupConfig",
            "params": [],
            "id": 1,
        });

        let response = client
            .post(format!("http://{}", addr))
            .json(&request_body)
            .send()
            .await
            .expect("Failed to send request");

        assert!(response.status().is_success());

        let rpc_response: RpcResponse = response.json().await.expect("Failed to parse response");

        let rpc_chain_config: ChainConfig = rpc_response.result.into();

        assert_eq!(config.chain.l2_genesis, rpc_chain_config.l2_genesis);
        assert_eq!(
            config.chain.l2_to_l1_message_passer,
            rpc_chain_config.l2_to_l1_message_passer
        );

        println!("{:#?}", rpc_chain_config);
        Ok(())
    }
}
