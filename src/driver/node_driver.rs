//! The node driver module contains the logic for advancing the execution node
//! by feeding the derived chain into the Engine API.

use std::{
    process,
    sync::{mpsc::Receiver, Arc, RwLock},
    time::Duration,
};

use alloy_primitives::Address;
use alloy_provider::ProviderBuilder;
use eyre::Result;
use reqwest::Url;
use tokio::{
    sync::watch::{self, Sender},
    time::sleep,
};

use crate::{
    common::{BlockInfo, Epoch},
    config::Config,
    derive::{state::State, Pipeline},
    engine::{Engine, EngineApi, ExecutionPayload},
    l1::{BlockUpdate, ChainWatcher},
    network::{handlers::block_handler::BlockHandler, service::Service},
    rpc,
    telemetry::metrics,
};

use crate::driver::{EngineDriver, HeadInfoFetcher, HeadInfoQuery};

/// NodeDriver is responsible for advancing the execution node by feeding
/// the derived chain into the engine API
pub struct NodeDriver<E: Engine> {
    /// The derivation pipeline
    pipeline: Pipeline,
    /// The engine driver
    engine_driver: EngineDriver<E>,
    /// List of unfinalized L2 blocks with their epochs, L1 inclusions, and sequence numbers
    unfinalized_blocks: Vec<(BlockInfo, Epoch, u64, u64)>,
    /// Current finalized L1 block number
    finalized_l1_block_number: u64,
    /// List of unsafe blocks that have not been applied yet
    future_unsafe_blocks: Vec<ExecutionPayload>,
    /// State struct to keep track of global state
    state: Arc<RwLock<State>>,
    /// L1 chain watcher
    chain_watcher: ChainWatcher,
    /// Channel to receive the shutdown signal from
    shutdown_recv: watch::Receiver<bool>,
    /// Channel to receive unsafe blocks from
    unsafe_block_recv: Receiver<ExecutionPayload>,
    /// Channel to send unsafe signer updates to block handler
    unsafe_block_signer_sender: Sender<Address>,
    /// Networking service
    network_service: Option<Service>,
    /// Channel timeout length
    channel_timeout: u64,
}

impl NodeDriver<EngineApi> {
    /// Creates a new [Driver] from the given [Config]
    pub async fn from_config(config: Config, shutdown_recv: watch::Receiver<bool>) -> Result<Self> {
        let url = Url::parse(&config.l2_rpc_url)?;
        let provider = ProviderBuilder::new().on_http(url);

        let head = HeadInfoQuery::get_head_info(&HeadInfoFetcher::from(&provider), &config).await;

        let finalized_head = head.l2_block_info;
        let finalized_epoch = head.l1_epoch;
        let finalized_seq = head.sequence_number;

        tracing::info!("starting from head: {:?}", finalized_head.hash);

        let l1_start_block =
            get_l1_start_block(finalized_epoch.number, config.chain.channel_timeout);

        let config = Arc::new(config);
        let chain_watcher =
            ChainWatcher::new(l1_start_block, finalized_head.number, config.clone())?;

        let state = State::new(finalized_head, finalized_epoch, &provider, config.clone()).await;
        let state = Arc::new(RwLock::new(state));

        let engine_driver = EngineDriver::new(finalized_head, finalized_epoch, provider, &config)?;
        let pipeline = Pipeline::new(state.clone(), config.clone(), finalized_seq)?;

        let _addr = rpc::run_server(config.clone()).await?;

        let signer = config.chain.system_config.unsafe_block_signer;
        let (unsafe_block_signer_sender, unsafe_block_signer_recv) = watch::channel(signer);

        let (block_handler, unsafe_block_recv) =
            BlockHandler::new(config.chain.l2_chain_id, unsafe_block_signer_recv);

        let service = Service::new("0.0.0.0:9876".parse()?, config.chain.l2_chain_id)
            .add_handler(Box::new(block_handler));

        Ok(Self {
            engine_driver,
            pipeline,
            unfinalized_blocks: Vec::new(),
            finalized_l1_block_number: 0,
            future_unsafe_blocks: Vec::new(),
            state,
            chain_watcher,
            shutdown_recv,
            unsafe_block_recv,
            unsafe_block_signer_sender,
            network_service: Some(service),
            channel_timeout: config.chain.channel_timeout,
        })
    }
}

impl<E: Engine> NodeDriver<E> {
    /// Runs the Driver
    pub async fn start(&mut self) -> Result<()> {
        self.await_engine_ready().await;
        self.chain_watcher.start()?;

        loop {
            self.check_shutdown().await;

            if let Err(err) = self.advance().await {
                tracing::error!("fatal error: {:?}", err);
                self.shutdown().await;
            }
        }
    }

    /// Shuts down the driver
    pub async fn shutdown(&self) {
        process::exit(0);
    }

    /// Checks for shutdown signal and shuts down if received
    async fn check_shutdown(&self) {
        if *self.shutdown_recv.borrow() {
            self.shutdown().await;
        }
    }

    /// Loops until the [EngineApi] is online and receives a response from the engine.
    async fn await_engine_ready(&self) {
        while !self.engine_driver.engine_ready().await {
            self.check_shutdown().await;
            sleep(Duration::from_secs(1)).await;
        }
    }

    /// Attempts to advance the execution node forward using either L1 info or
    /// blocks received on the p2p network.
    async fn advance(&mut self) -> Result<()> {
        self.advance_safe_head().await?;
        self.advance_unsafe_head().await?;

        self.update_finalized();
        self.update_metrics();
        self.try_start_networking()?;

        Ok(())
    }

    /// Attempts to advance the execution node forward one L1 block using derived
    /// L1 data. Errors if the most recent PayloadAttributes from the pipeline
    /// does not successfully advance the node
    async fn advance_safe_head(&mut self) -> Result<()> {
        self.handle_next_block_update().await?;
        self.update_state_head()?;

        for next_attributes in self.pipeline.by_ref() {
            let l1_inclusion_block = next_attributes
                .l1_inclusion_block
                .ok_or(eyre::eyre!("attributes without inclusion block"))?;

            let seq_number = next_attributes
                .seq_number
                .ok_or(eyre::eyre!("attributes without seq number"))?;

            self.engine_driver
                .handle_attributes(next_attributes)
                .await
                .map_err(|e| eyre::eyre!("failed to handle attributes: {}", e))?;

            tracing::info!(
                "safe head updated: {} {:?}",
                self.engine_driver.safe_head.number,
                self.engine_driver.safe_head.hash,
            );

            let new_safe_head = self.engine_driver.safe_head;
            let new_safe_epoch = self.engine_driver.safe_epoch;

            self.state
                .write()
                .map_err(|_| eyre::eyre!("lock poisoned"))?
                .update_safe_head(new_safe_head, new_safe_epoch);

            let unfinalized_entry = (
                new_safe_head,
                new_safe_epoch,
                l1_inclusion_block,
                seq_number,
            );

            self.unfinalized_blocks.push(unfinalized_entry);
        }

        Ok(())
    }

    /// Collects unsafe blocks received via p2p gossip and updates the forkchoice with the first available unsafe block.
    async fn advance_unsafe_head(&mut self) -> Result<()> {
        while let Ok(payload) = self.unsafe_block_recv.try_recv() {
            self.future_unsafe_blocks.push(payload);
        }

        self.future_unsafe_blocks.retain(|payload| {
            let unsafe_block_num: u64 = match payload.block_number.try_into() {
                Ok(num) => num,
                Err(_) => return false,
            };
            let synced_block_num = self.engine_driver.unsafe_head.number;

            unsafe_block_num > synced_block_num && unsafe_block_num - synced_block_num < 1024
        });

        let next_unsafe_payload = self
            .future_unsafe_blocks
            .iter()
            .find(|p| p.parent_hash == self.engine_driver.unsafe_head.hash);

        if let Some(payload) = next_unsafe_payload {
            _ = self.engine_driver.handle_unsafe_payload(payload).await;
        }

        Ok(())
    }

    /// Updates the [State] `safe_head`
    fn update_state_head(&self) -> Result<()> {
        let mut state = self
            .state
            .write()
            .map_err(|_| eyre::eyre!("lock poisoned"))?;

        state.update_safe_head(self.engine_driver.safe_head, self.engine_driver.safe_epoch);

        Ok(())
    }

    /// Ingests the next update from the block update channel
    async fn handle_next_block_update(&mut self) -> Result<()> {
        let next = self.chain_watcher.try_recv_from_channel();

        if let Ok(update) = next {
            match update {
                BlockUpdate::NewBlock(l1_info) => {
                    let num = l1_info.block_info.number;

                    let signer =
                        Address::from_slice(l1_info.system_config.unsafe_block_signer.as_slice());
                    self.unsafe_block_signer_sender.send(signer)?;

                    self.pipeline.push_batcher_transactions(
                        // cloning `bytes::Bytes` is cheap
                        l1_info.batcher_transactions.clone(),
                        num,
                    )?;

                    self.state
                        .write()
                        .map_err(|_| eyre::eyre!("lock poisoned"))?
                        .update_l1_info(*l1_info);
                }
                BlockUpdate::Reorg => {
                    tracing::warn!("reorg detected, purging pipeline");

                    self.unfinalized_blocks.clear();

                    let l1_start_block = get_l1_start_block(
                        self.engine_driver.finalized_epoch.number,
                        self.channel_timeout,
                    );

                    self.chain_watcher
                        .restart(l1_start_block, self.engine_driver.finalized_head.number)?;

                    self.state
                        .write()
                        .map_err(|_| eyre::eyre!("lock poisoned"))?
                        .purge(
                            self.engine_driver.finalized_head,
                            self.engine_driver.finalized_epoch,
                        );

                    self.pipeline.purge()?;
                    self.engine_driver.reorg();
                }
                BlockUpdate::FinalityUpdate(num) => {
                    self.finalized_l1_block_number = num;
                }
            }
        }

        Ok(())
    }

    /// Updates the current finalized L2 block in the [EngineDriver] based on their inclusion in finalized L1 blocks
    fn update_finalized(&mut self) {
        let new_finalized = self
            .unfinalized_blocks
            .iter()
            .filter(|(_, _, inclusion, seq)| {
                *inclusion <= self.finalized_l1_block_number && *seq == 0
            })
            .last();

        if let Some((head, epoch, _, _)) = new_finalized {
            self.engine_driver.update_finalized(*head, *epoch);
        }

        self.unfinalized_blocks
            .retain(|(_, _, inclusion, _)| *inclusion > self.finalized_l1_block_number);
    }

    /// Begins p2p networking if fully synced with no unfinalized blocks
    fn try_start_networking(&mut self) -> Result<()> {
        if self.synced() {
            if let Some(service) = self.network_service.take() {
                service.start()?;
            }
        }

        Ok(())
    }

    /// Updates Prometheus metrics
    fn update_metrics(&self) {
        metrics::FINALIZED_HEAD.set(self.engine_driver.finalized_head.number as i64);
        metrics::SAFE_HEAD.set(self.engine_driver.safe_head.number as i64);
        metrics::SYNCED.set(self.synced() as i64);
    }

    /// True if there are no unfinalized blocks
    fn synced(&self) -> bool {
        !self.unfinalized_blocks.is_empty()
    }
}

/// Retrieves the L1 start block number.
/// If an overflow occurs during subtraction, the function returns the genesis block #0.
fn get_l1_start_block(epoch_number: u64, channel_timeout: u64) -> u64 {
    epoch_number.saturating_sub(channel_timeout)
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, str::FromStr};

    use alloy_provider::Provider;
    use alloy_rpc_types::{BlockId, BlockNumberOrTag};
    use eyre::Result;
    use tokio::sync::watch::channel;

    use crate::config::{ChainConfig, CliConfig};

    use super::*;

    #[tokio::test]
    async fn test_new_driver_from_finalized_head() -> Result<()> {
        if std::env::var("L1_TEST_RPC_URL").is_ok() && std::env::var("L2_TEST_RPC_URL").is_ok() {
            let config_path = PathBuf::from_str("config.toml")?;
            let rpc = std::env::var("L1_TEST_RPC_URL")?;
            let l2_rpc = std::env::var("L2_TEST_RPC_URL")?;
            let cli_config = CliConfig {
                l1_rpc_url: Some(rpc.to_owned()),
                l1_beacon_url: None,
                l2_rpc_url: Some(l2_rpc.to_owned()),
                l2_engine_url: None,
                jwt_secret: Some(
                    "d195a64e08587a3f1560686448867220c2727550ce3e0c95c7200d0ade0f9167".to_owned(),
                ),
                checkpoint_sync_url: Some(l2_rpc.to_owned()),
                rpc_port: None,
                rpc_addr: None,
                devnet: false,
            };
            let config = Config::new(&config_path, cli_config, ChainConfig::optimism_goerli());
            let (_shutdown_sender, shutdown_recv) = channel(false);

            let block_id = BlockId::Number(BlockNumberOrTag::Finalized);
            let url = Url::parse(&config.l2_rpc_url)?;
            let provider = ProviderBuilder::new().on_http(url);
            let finalized_block = provider.get_block(block_id, true).await?.unwrap();

            let driver = NodeDriver::from_config(config, shutdown_recv).await?;

            assert_eq!(
                driver.engine_driver.finalized_head.number,
                finalized_block.header.number.unwrap()
            );
        }
        Ok(())
    }
}
