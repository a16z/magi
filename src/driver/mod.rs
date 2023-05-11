use std::{
    process,
    sync::{mpsc::Receiver, Arc, RwLock},
    time::Duration,
};

use ethers::{
    providers::{Middleware, Provider},
    types::{BlockId, BlockNumber},
};
use eyre::Result;
use tokio::time::sleep;

use crate::{
    common::{BlockInfo, Epoch},
    config::Config,
    derive::{state::State, Pipeline},
    driver::types::HeadInfo,
    engine::{Engine, EngineApi, ExecutionPayload},
    l1::{BlockUpdate, ChainWatcher},
    network::{handlers::block_handler::BlockHandler, service::Service},
    rpc,
    telemetry::metrics,
};

use self::engine_driver::EngineDriver;

mod engine_driver;
mod types;

/// Driver is responsible for advancing the execution node by feeding
/// the derived chain into the engine API
pub struct Driver<E: Engine> {
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
    shutdown_recv: Arc<Receiver<()>>,
    /// Channel to receive unsafe block from
    unsafe_block_recv: Receiver<ExecutionPayload>,
}

impl Driver<EngineApi> {
    pub async fn from_config(config: Config, shutdown_recv: Arc<Receiver<()>>) -> Result<Self> {
        let provider = Provider::try_from(&config.l2_rpc_url)?;

        let block_id = BlockId::Number(BlockNumber::Finalized);
        let finalized_block = provider.get_block_with_txs(block_id).await?;

        let head = finalized_block
            .and_then(|block| HeadInfo::try_from(block).ok())
            .unwrap_or_else(|| {
                tracing::warn!("could not get head info. Falling back to the genesis head.");
                HeadInfo {
                    l2_block_info: config.chain.l2_genesis,
                    l1_epoch: config.chain.l1_start_epoch,
                }
            });

        let finalized_head = head.l2_block_info;
        let finalized_epoch = head.l1_epoch;

        tracing::info!("starting from head: {:?}", finalized_head.hash);

        let config = Arc::new(config);
        let chain_watcher = ChainWatcher::new(
            finalized_epoch.number,
            finalized_head.number,
            config.clone(),
        )?;

        let state = Arc::new(RwLock::new(State::new(
            finalized_head,
            finalized_epoch,
            config.clone(),
        )));

        let engine_driver = EngineDriver::new(finalized_head, finalized_epoch, provider, &config)?;
        let pipeline = Pipeline::new(state.clone(), config.clone())?;

        let _addr = rpc::run_server(config.clone()).await?;

        let (block_handler, unsafe_block_recv) = BlockHandler::new(config.chain.chain_id);

        Service::new("0.0.0.0:9876".parse()?, config.chain.chain_id)
            .add_handler(Box::new(block_handler))
            .start()?;

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
        })
    }
}

impl<E: Engine> Driver<E> {
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
        if self.shutdown_recv.try_recv().is_ok() {
            self.shutdown().await;
        }
    }

    async fn await_engine_ready(&self) {
        while !self.engine_driver.engine_ready().await {
            self.check_shutdown().await;
            sleep(Duration::from_secs(1)).await;
        }
    }

    /// Attempts to advance the execution node forward using either L1 info our
    /// blocks received on the p2p network.
    async fn advance(&mut self) -> Result<()> {
        self.advance_safe_head().await?;
        self.advance_unsafe_head().await?;

        self.update_finalized();
        self.update_metrics();

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
                .await?;

            tracing::info!(
                "safe head updated: {} {:?}",
                self.engine_driver.safe_head.number,
                self.engine_driver.safe_head.hash,
            );

            let new_safe_head = self.engine_driver.safe_head;
            let new_safe_epoch = self.engine_driver.safe_epoch;

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

    async fn advance_unsafe_head(&mut self) -> Result<()> {
        while let Ok(payload) = self.unsafe_block_recv.try_recv() {
            self.future_unsafe_blocks.push(payload);
            self.future_unsafe_blocks.retain(|payload| {
                let unsafe_block_num = payload.block_number.as_u64();
                let synced_block_num = self.engine_driver.unsafe_head.number;

                unsafe_block_num > synced_block_num && unsafe_block_num - synced_block_num < 256
            });

            let next_unsafe_payload = self
                .future_unsafe_blocks
                .iter()
                .find(|p| p.parent_hash == self.engine_driver.unsafe_head.hash);

            if let Some(payload) = next_unsafe_payload {
                _ = self.engine_driver.handle_unsafe_payload(payload).await;
            }
        }

        Ok(())
    }

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
        let is_state_full = self
            .state
            .read()
            .map_err(|_| eyre::eyre!("lock poisoned"))?
            .is_full();

        if !is_state_full {
            let next = self.chain_watcher.block_update_receiver.try_recv();

            if let Ok(update) = next {
                match update {
                    BlockUpdate::NewBlock(l1_info) => {
                        let num = l1_info.block_info.number;
                        self.pipeline
                            .push_batcher_transactions(l1_info.batcher_transactions.clone(), num)?;

                        self.state
                            .write()
                            .map_err(|_| eyre::eyre!("lock poisoned"))?
                            .update_l1_info(*l1_info);
                    }
                    BlockUpdate::Reorg => {
                        tracing::warn!("reorg detected, purging pipeline");

                        self.unfinalized_blocks.clear();
                        self.chain_watcher.restart(
                            self.engine_driver.finalized_epoch.number,
                            self.engine_driver.finalized_head.number,
                        )?;

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
        }

        Ok(())
    }

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

    fn update_metrics(&self) {
        metrics::FINALIZED_HEAD.set(self.engine_driver.finalized_head.number as i64);
        metrics::SAFE_HEAD.set(self.engine_driver.safe_head.number as i64);
        metrics::SYNCED.set(!self.unfinalized_blocks.is_empty() as i64);
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, str::FromStr, sync::mpsc::channel};

    use ethers::providers::{Http, Middleware};
    use eyre::Result;

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
                l2_rpc_url: Some(l2_rpc.to_owned()),
                l2_engine_url: None,
                jwt_secret: Some(
                    "d195a64e08587a3f1560686448867220c2727550ce3e0c95c7200d0ade0f9167".to_owned(),
                ),
                checkpoint_sync_url: Some(l2_rpc.to_owned()),
                rpc_port: None,
            };
            let config = Config::new(&config_path, cli_config, ChainConfig::optimism_goerli());
            let (_shutdown_sender, shutdown_recv) = channel();

            let block_id = BlockId::Number(BlockNumber::Finalized);
            let provider = Provider::<Http>::try_from(config.l2_rpc_url.clone())?;
            let finalized_block = provider.get_block(block_id).await?.unwrap();

            let driver = Driver::from_config(config, Arc::new(shutdown_recv)).await?;

            assert_eq!(
                driver.engine_driver.finalized_head.number,
                finalized_block.number.unwrap().as_u64()
            );
        }
        Ok(())
    }
}
