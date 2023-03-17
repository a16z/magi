use std::{
    process,
    sync::{
        mpsc::{channel, Receiver},
        Arc, RwLock,
    },
    thread::sleep,
    time::Duration,
};

use async_recursion::async_recursion;
use ethers_core::{
    types::{Block, H256},
    utils::keccak256,
};
use ethers_providers::{Middleware, Provider};
use eyre::Result;
use tokio::spawn;

use crate::{
    backend::{Database, HeadInfo},
    common::{BlockInfo, Epoch},
    config::Config,
    derive::{state::State, Pipeline},
    engine::{
        EngineApi, ExecutionPayload, ForkchoiceState, L2EngineApi, PayloadAttributes, Status,
    },
    l1::{BlockUpdate, ChainWatcher},
};

/// Driver is responsible for advancing the execution node by feeding
/// the derived chain into the engine API
pub struct Driver<E: L2EngineApi> {
    /// The derivation pipeline
    pipeline: Pipeline,
    /// The L2 execution engine
    engine: Arc<E>,
    /// Database for storing progress data
    db: Database,
    /// Most recent block hash that can be derived from L1 data
    pub safe_head: BlockInfo,
    /// Batch epoch of the safe head
    safe_epoch: Epoch,
    /// Most recent block hash that can be derived from finalized L1 data
    pub finalized_head: BlockInfo,
    /// Batch epoch of the finalized head
    finalized_epoch: Epoch,
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
    /// Global config
    config: Arc<Config>,
}

impl Driver<EngineApi> {
    pub fn from_config(config: Config, shutdown_recv: Receiver<bool>) -> Result<Self> {
        let db = config
            .data_dir
            .as_ref()
            .map(Database::new)
            .unwrap_or_default();

        let head = db.read_head();

        let finalized_head = head
            .as_ref()
            .map(|h| prev_block_id(&h.l2_block_info))
            .unwrap_or(config.chain.l2_genesis);

        let finalized_epoch = head
            .map(|h| h.l1_epoch)
            .unwrap_or(config.chain.l1_start_epoch);

        tracing::info!("syncing from: {:?}", finalized_head.hash);

        let config = Arc::new(config);
        let chain_watcher = ChainWatcher::new(finalized_epoch.number, config.clone())?;

        let safe_head = finalized_head;
        let safe_epoch = finalized_epoch;

        let state = Arc::new(RwLock::new(State::new(
            safe_head,
            safe_epoch,
            config.clone(),
        )));

        let engine = Arc::new(EngineApi::new(
            config.engine_api_url.clone().unwrap_or_default(),
            config.jwt_secret.clone(),
        ));

        let pipeline = Pipeline::new(state.clone(), config.clone())?;

        Ok(Self {
            db,
            engine,
            pipeline,
            safe_head,
            safe_epoch,
            finalized_head,
            finalized_epoch,
            unfinalized_origins: Vec::new(),
            finalized_l1_block_number: 0,
            state,
            chain_watcher,
            shutdown_recv,
            config,
        })
    }
}

impl<E: L2EngineApi> Driver<E> {
    /// Creates a new Driver instance
    pub fn from_internals(engine: E, pipeline: Pipeline, config: Arc<Config>) -> Result<Self> {
        let finalized_head = config.chain.l2_genesis;
        let finalized_epoch = config.chain.l1_start_epoch;

        let safe_head = finalized_head;
        let safe_epoch = finalized_epoch;

        let chain_watcher = ChainWatcher::new(finalized_epoch.number, config.clone())?;

        let state = Arc::new(RwLock::new(State::new(
            safe_head,
            safe_epoch,
            config.clone(),
        )));

        let db = Database::default();
        let (_, shutdown_recv) = channel();

        Ok(Self {
            pipeline,
            engine: Arc::new(engine),
            db,
            safe_head,
            safe_epoch,
            finalized_head,
            finalized_epoch,
            unfinalized_origins: Vec::new(),
            finalized_l1_block_number: 0,
            state,
            chain_watcher,
            shutdown_recv,
            config,
        })
    }

    /// Runs the Driver
    pub async fn start(&mut self) -> Result<()> {
        self.skip_common_blocks().await?;

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
            self.process_attributes(next_attributes).await?;
        }

        Ok(())
    }

    async fn process_attributes(&mut self, attributes: PayloadAttributes) -> Result<()> {
        tracing::debug!(target: "magi", "received new attributes from the pipeline");
        tracing::debug!("next attributes: {:?}", attributes);

        let new_epoch = *attributes.epoch.as_ref().unwrap();

        let l1_origin = attributes
            .l1_origin
            .ok_or(eyre::eyre!("attributes without origin"))?;

        let seq_number = attributes
            .seq_number
            .ok_or(eyre::eyre!("attributes without sequencer number"))?;

        let payload = self.build_payload(attributes).await?;

        let new_head = BlockInfo {
            number: payload.block_number.as_u64(),
            hash: payload.block_hash,
            parent_hash: payload.parent_hash,
            timestamp: payload.timestamp.as_u64(),
        };

        self.push_payload(payload).await?;
        self.update_forkchoice(new_head);

        self.update_head(new_head, new_epoch)?;

        self.unfinalized_origins
            .push((new_head, new_epoch, l1_origin, seq_number));

        self.update_finalized();

        tracing::info!(target: "magi", "head updated: {} {:?}", self.safe_head.number, self.safe_head.hash);

        Ok(())
    }

    fn update_state_head(&self) -> Result<()> {
        let mut state = self
            .state
            .write()
            .map_err(|_| eyre::eyre!("lock poisoned"))?;

        state.update_safe_head(self.safe_head, self.safe_epoch);

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
                        self.chain_watcher.reset(self.finalized_epoch.number)?;

                        self.state
                            .write()
                            .map_err(|_| eyre::eyre!("lock poisoned"))?
                            .purge(self.finalized_head, self.finalized_epoch);

                        self.pipeline.purge()?;

                        self.safe_head = self.finalized_head;
                        self.safe_epoch = self.finalized_epoch;

                        self.skip_common_blocks().await?;
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
            .find(|(_, _, origin, seq)| *origin <= self.finalized_l1_block_number && *seq == 0);

        if let Some((head, epoch, _, _)) = new_finalized {
            tracing::info!("saving new finalized head to db: {:?}", head.hash);

            let res = self.db.write_head(HeadInfo {
                l2_block_info: *head,
                l1_epoch: *epoch,
            });

            if res.is_ok() {
                self.finalized_head = *head;
                self.finalized_epoch = *epoch;
            }
        }

        self.unfinalized_origins
            .retain(|(_, _, origin, _)| *origin > self.finalized_l1_block_number);
    }

    async fn build_payload(&self, attributes: PayloadAttributes) -> Result<ExecutionPayload> {
        let forkchoice = create_forkchoice_state(self.safe_head.hash, self.finalized_head.hash);

        let update = self
            .engine
            .forkchoice_updated(forkchoice, Some(attributes))
            .await?;

        if update.payload_status.status != Status::Valid {
            eyre::bail!("invalid payload attributes");
        }

        let id = update
            .payload_id
            .ok_or(eyre::eyre!("engine did not return payload id"))?;

        self.engine.get_payload(id).await
    }

    async fn push_payload(&self, payload: ExecutionPayload) -> Result<()> {
        let status = self.engine.new_payload(payload).await?;
        if status.status != Status::Valid && status.status != Status::Accepted {
            eyre::bail!("invalid execution payload");
        }

        Ok(())
    }

    fn update_forkchoice(&self, new_head: BlockInfo) {
        let forkchoice = create_forkchoice_state(new_head.hash, self.finalized_head.hash);
        let engine = self.engine.clone();

        spawn(async move {
            let update = engine.forkchoice_updated(forkchoice, None).await?;
            if update.payload_status.status != Status::Valid {
                eyre::bail!(
                    "could not accept new forkchoice: {:?}",
                    update.payload_status.validation_error
                );
            }

            Ok(())
        });
    }

    fn update_head(&mut self, new_head: BlockInfo, new_epoch: Epoch) -> Result<()> {
        if self.safe_head != new_head {
            self.safe_head = new_head;
            self.safe_epoch = new_epoch;
        }

        Ok(())
    }

    /// Advances the pipeline until it finds a payload attributes that the engine has not processed
    /// in the past. This should be called whenever the pipeline starts up or reorgs. Returns once
    /// it finds a new payload attributes.
    #[async_recursion(?Send)]
    async fn skip_common_blocks(&mut self) -> Result<()> {
        let provider = Provider::try_from(
            self.config
                .l2_rpc_url
                .as_ref()
                .ok_or(eyre::eyre!("L2 RPC not found"))?,
        )?;

        loop {
            self.check_shutdown().await;
            self.handle_next_block_update().await?;
            self.update_state_head()?;

            let payload = self.pipeline.peak();
            let block = provider.get_block(self.safe_head.number + 1).await?;

            match (payload, block) {
                (Some(payload), Some(block)) => {
                    let payload_hashes = payload
                        .transactions
                        .as_ref()
                        .unwrap()
                        .iter()
                        .map(|tx| H256(keccak256(&tx.0)))
                        .collect::<Vec<_>>();

                    let is_same = payload_hashes == block.transactions
                        && payload.timestamp.as_u64() == block.timestamp.as_u64()
                        && payload.prev_randao == block.mix_hash.unwrap()
                        && payload.suggested_fee_recipient == block.author.unwrap()
                        && payload.gas_limit.as_u64() == block.gas_limit.as_u64();

                    if is_same {
                        tracing::info!("skipping already processed block");
                        if let Some(payload) = self.pipeline.next() {
                            self.skip_attributes(payload, block)?;
                        }
                    } else {
                        return Ok(());
                    }
                }
                (Some(_), None) => {
                    return Ok(());
                }
                _ => sleep(Duration::from_millis(100)),
            };
        }
    }

    fn skip_attributes(&mut self, attributes: PayloadAttributes, block: Block<H256>) -> Result<()> {
        let new_epoch = *attributes.epoch.as_ref().unwrap();

        let l1_origin = attributes
            .l1_origin
            .ok_or(eyre::eyre!("attributes without origin"))?;

        let seq_number = attributes
            .seq_number
            .ok_or(eyre::eyre!("attributes without sequencer number"))?;

        let new_head = BlockInfo {
            number: block
                .number
                .ok_or(eyre::eyre!("block not included"))?
                .as_u64(),
            hash: block.hash.ok_or(eyre::eyre!("block not included"))?,
            parent_hash: block.parent_hash,
            timestamp: block.timestamp.as_u64(),
        };

        self.update_head(new_head, new_epoch)?;

        self.unfinalized_origins
            .push((new_head, new_epoch, l1_origin, seq_number));

        self.update_finalized();

        tracing::info!(target: "magi", "head updated: {} {:?}", self.safe_head.number, self.safe_head.hash);

        Ok(())
    }
}

fn create_forkchoice_state(safe_hash: H256, finalized_hash: H256) -> ForkchoiceState {
    ForkchoiceState {
        head_block_hash: safe_hash,
        safe_block_hash: safe_hash,
        finalized_block_hash: finalized_hash,
    }
}

fn prev_block_id(block: &BlockInfo) -> BlockInfo {
    BlockInfo {
        number: block.number - 1,
        hash: block.parent_hash,
        parent_hash: H256::zero(),
        timestamp: block.timestamp - 2,
    }
}
