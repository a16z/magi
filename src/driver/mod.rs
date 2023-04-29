use std::{
    process,
    sync::{mpsc::Receiver, Arc, RwLock},
    time::Duration,
};

use ethers::{
    providers::{Http, Middleware, Provider},
    types::{BlockId, BlockNumber, SyncingStatus, H256},
};
use eyre::Result;
use tokio::time::sleep;

use crate::{
    common::{BlockInfo, Epoch},
    config::Config,
    derive::{state::State, Pipeline},
    driver::types::HeadInfo,
    engine::{Engine, EngineApi, ExecutionPayload, ForkchoiceState, Status},
    l1::{BlockUpdate, ChainWatcher},
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
    /// State struct to keep track of global state
    state: Arc<RwLock<State>>,
    /// L1 chain watcher
    chain_watcher: ChainWatcher,
    /// Channel to receive the shutdown signal from
    shutdown_recv: Receiver<bool>,
}

impl Driver<EngineApi> {
    pub async fn from_finalized_head(
        config: Config,
        shutdown_recv: Receiver<bool>,
    ) -> Result<Self> {
        let provider = Provider::try_from(&config.l2_rpc_url)?;

        let block_id = BlockId::Number(BlockNumber::Finalized);
        let head = match HeadInfo::from_block(block_id, &provider).await {
            Ok(Some(head)) => head,
            _ => {
                tracing::warn!("could not get head info. Falling back to the genesis head.");
                HeadInfo {
                    l2_block_info: config.chain.l2_genesis,
                    l1_epoch: config.chain.l1_start_epoch,
                }
            }
        };

        let finalized_head = head.l2_block_info;
        let finalized_epoch = head.l1_epoch;

        tracing::info!("syncing from: {:?}", finalized_head.hash);

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

        let _addr = rpc::run_server(config).await?;

        Ok(Self {
            engine_driver,
            pipeline,
            unfinalized_blocks: Vec::new(),
            finalized_l1_block_number: 0,
            state,
            chain_watcher,
            shutdown_recv,
        })
    }

    pub async fn from_checkpoint_hash(
        config: Config,
        shutdown_recv: Receiver<bool>,
        checkpoint_hash: H256,
    ) -> Result<Self> {
        let provider = Provider::try_from(&config.l2_rpc_url)?;
        let config = Arc::new(config);

        let engine_api = EngineApi::new(&config.l2_engine_url, &config.jwt_secret);

        // built the execution payload of the checkpoint block and send it to the execution client
        let checkpoint_payload = ExecutionPayload::from_block(&config, checkpoint_hash).await?;
        let payload_res = engine_api.new_payload(checkpoint_payload).await?;
        if let Status::Invalid | Status::InvalidBlockHash = payload_res.status {
            tracing::error!("the provided checkpoint payload is invalid, exiting");
            process::exit(1);
        }

        // call forkchoice_updated once to make the execution layer start syncing to the checkpoint
        let forkchoice_state = ForkchoiceState::from_single_head(checkpoint_hash);
        let forkchoice_res = engine_api
            .forkchoice_updated(forkchoice_state, None)
            .await?;
        if let Status::Invalid | Status::InvalidBlockHash = forkchoice_res.payload_status.status {
            tracing::error!("could not accept forkchoice, exiting");
            process::exit(1);
        }

        tracing::info!(
            "syncing execution client up to checkpoint: {:?}",
            checkpoint_hash
        );

        // wait until the execution layer has synced to the checkpoint
        await_syncing(&provider, &shutdown_recv).await?;

        // now that we've synced, we can get the head info for the checkpoint block
        // (our l2 rpc should now have this block)
        let head = match HeadInfo::from_block(BlockId::Hash(checkpoint_hash), &provider).await {
            Ok(Some(head)) => head,
            _ => {
                tracing::warn!("could not get head info. Falling back to the genesis head.");
                HeadInfo {
                    l2_block_info: config.chain.l2_genesis,
                    l1_epoch: config.chain.l1_start_epoch,
                }
            }
        };

        let finalized_head = head.l2_block_info;
        let finalized_epoch = head.l1_epoch;

        tracing::info!("starting  from: {:?}", finalized_head.hash);

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
        let pipeline = Pipeline::new(state.clone(), config)?;

        Ok(Self {
            engine_driver,
            pipeline,
            unfinalized_blocks: Vec::new(),
            finalized_l1_block_number: 0,
            state,
            chain_watcher,
            shutdown_recv,
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

    pub async fn start_fast(&mut self) -> Result<()> {
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
        if let Ok(shutdown) = self.shutdown_recv.try_recv() {
            if shutdown {
                self.shutdown().await;
            }
        }
    }

    async fn await_engine_ready(&self) {
        while !self.engine_driver.engine_ready().await {
            self.check_shutdown().await;
            sleep(Duration::from_secs(1)).await;
        }
    }

    /// Attempts to advance the execution node forward one L1 block using derived
    /// L1 data. Errors if the most recent PayloadAttributes from the pipeline
    /// does not successfully advance the node
    async fn advance(&mut self) -> Result<()> {
        self.handle_next_block_update().await?;
        self.update_state_head()?;

        while let Some(next_attributes) = self.pipeline.next() {
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
                "head updated: {} {:?}",
                self.engine_driver.safe_head.number,
                self.engine_driver.safe_head.hash
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
            self.update_finalized();

            self.update_metrics();
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

async fn await_syncing(provider: &Provider<Http>, shutdown_recv: &Receiver<bool>) -> Result<()> {
    loop {
        if let Ok(shutdown) = shutdown_recv.try_recv() {
            if shutdown {
                process::exit(0);
            }
        }

        match &provider.syncing().await? {
            SyncingStatus::IsSyncing(progress) => {
                tracing::debug!(
                    "syncing progress: {} / {}",
                    progress.current_block,
                    progress.highest_block
                );
                sleep(Duration::from_secs(2)).await;
            }
            SyncingStatus::IsFalse => {
                tracing::info!("syncing complete");
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, str::FromStr, sync::mpsc::channel};

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
                l2_trusted_rpc_url: Some(l2_rpc.to_owned()),
                rpc_port: None,
            };
            let config = Config::new(&config_path, cli_config, ChainConfig::optimism_goerli());
            let (_shutdown_sender, shutdown_recv) = channel();

            let block_id = BlockId::Number(BlockNumber::Finalized);
            let provider = Provider::<Http>::try_from(config.l2_rpc_url.clone())?;
            let finalized_block = provider.get_block(block_id).await?.unwrap();

            let driver = Driver::from_finalized_head(config, shutdown_recv).await?;

            assert_eq!(
                driver.engine_driver.finalized_head.number,
                finalized_block.number.unwrap().as_u64()
            );
        }
        Ok(())
    }
}
