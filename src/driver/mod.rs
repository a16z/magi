use std::{
    process,
    sync::{mpsc::Receiver, Arc, RwLock},
};

use eyre::Result;

use crate::{
    backend::{Database, HeadInfo},
    common::{BlockInfo, Epoch},
    config::Config,
    derive::{state::State, Pipeline},
    engine::{Engine, EngineApi},
    l1::{BlockUpdate, ChainWatcher},
    telemetry::metrics,
};

use self::engine_driver::EngineDriver;

mod engine_driver;

/// Driver is responsible for advancing the execution node by feeding
/// the derived chain into the engine API
pub struct Driver<E: Engine> {
    /// The derivation pipeline
    pipeline: Pipeline,
    /// The engine driver
    engine_driver: EngineDriver<E>,
    /// Database for storing progress data
    db: Database,
    /// List of unfinalized L2 blocks with their epochs, L1 origin, and sequence numbers
    unfinalized_origins: Vec<(BlockInfo, Epoch, u64, u64)>,
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
    pub fn from_config(config: Config, shutdown_recv: Receiver<bool>) -> Result<Self> {
        let db = config
            .data_dir
            .as_ref()
            .map(|dir| Database::new(dir, &config.chain.network))
            .unwrap_or_default();

        let head = db.read_head();

        let finalized_head = head
            .as_ref()
            .map(|h| h.l2_block_info)
            .unwrap_or(config.chain.l2_genesis);

        let finalized_epoch = head
            .map(|h| h.l1_epoch)
            .unwrap_or(config.chain.l1_start_epoch);

        tracing::info!("syncing from: {:?}", finalized_head.hash);

        let config = Arc::new(config);
        let chain_watcher = ChainWatcher::new(finalized_epoch.number, config.clone())?;

        let state = Arc::new(RwLock::new(State::new(
            finalized_head,
            finalized_epoch,
            config.clone(),
        )));

        let engine_driver = EngineDriver::new(finalized_head, finalized_epoch, &config)?;
        let pipeline = Pipeline::new(state.clone(), config)?;

        Ok(Self {
            db,
            engine_driver,
            pipeline,
            unfinalized_origins: Vec::new(),
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
        self.engine_driver.wait_engine_ready().await;

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
        let size = self.db.flush_async().await.expect("could not flush db");
        tracing::info!(target: "magi::driver", "flushed {} bytes to disk", size);
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

    /// Attempts to advance the execution node forward one L1 block using derived
    /// L1 data. Errors if the most recent PayloadAttributes from the pipeline
    /// does not successfully advance the node
    pub async fn advance(&mut self) -> Result<()> {
        self.handle_next_block_update().await?;
        self.update_state_head()?;

        while let Some(next_attributes) = self.pipeline.next() {
            let l1_origin = next_attributes
                .l1_origin
                .ok_or(eyre::eyre!("attributes without origin"))?;

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

            let unfinalized_entry = (new_safe_head, new_safe_epoch, l1_origin, seq_number);
            self.unfinalized_origins.push(unfinalized_entry);
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

                        self.unfinalized_origins.clear();
                        self.chain_watcher
                            .reset(self.engine_driver.finalized_epoch.number)?;

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
            .unfinalized_origins
            .iter()
            .filter(|(_, _, origin, seq)| *origin <= self.finalized_l1_block_number && *seq == 0)
            .last();

        if let Some((head, epoch, _, _)) = new_finalized {
            tracing::info!("saving new finalized head to db: {:?}", head.hash);

            let res = self.db.write_head(HeadInfo {
                l2_block_info: *head,
                l1_epoch: *epoch,
            });

            if res.is_ok() {
                self.engine_driver.update_finalized(*head, *epoch);
            }
        }

        self.unfinalized_origins
            .retain(|(_, _, origin, _)| *origin > self.finalized_l1_block_number);
    }

    fn update_metrics(&self) {
        metrics::FINALIZED_HEAD.set(self.engine_driver.finalized_head.number as i64);
        metrics::SAFE_HEAD.set(self.engine_driver.safe_head.number as i64);
        metrics::SYNCED.set(if self.unfinalized_origins.is_empty() {
            0
        } else {
            1
        });
    }
}
