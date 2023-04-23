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
    engine::{Engine, EngineApi},
    l1::{BlockUpdate, ChainWatcher},
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
    pub async fn from_config(config: Config, shutdown_recv: Receiver<bool>) -> Result<Self> {
        let provider = Provider::try_from(&config.l2_rpc_url)?;

        let head: Option<HeadInfo> = if let Some(block) = provider
            .get_block_with_txs(BlockId::Number(BlockNumber::Finalized))
            .await?
        {
            Some(block.try_into()?)
        } else {
            None
        };

        let finalized_head = head
            .as_ref()
            .map(|h| h.l2_block_info)
            .unwrap_or(config.chain.l2_genesis);

        let finalized_epoch = head
            .map(|h| h.l1_epoch)
            .unwrap_or(config.chain.l1_start_epoch);

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
                tracing::error!(target: "magi", "fatal error: {:?}", err);
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
                target: "magi",
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
