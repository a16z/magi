use std::sync::{Arc, RwLock};

use ethers_core::types::H256;
use eyre::Result;

use crate::{
    backend::{Database, HeadInfo},
    common::{BlockInfo, Epoch},
    config::Config,
    derive::{state::State, Pipeline},
    engine::{
        EngineApi, ExecutionPayload, ForkchoiceState, L2EngineApi, PayloadAttributes, Status,
    },
    l1::ChainWatcher,
};

/// Driver is responsible for advancing the execution node by feeding
/// the derived chain into the engine API
pub struct Driver<E: L2EngineApi, P: Iterator<Item = PayloadAttributes>> {
    /// The derivation pipeline
    pipeline: P,
    /// The L2 execution engine
    engine: E,
    /// Database for storing progress data
    db: Database,
    /// Most recent block hash that can be derived from L1 data
    pub safe_head: BlockInfo,
    /// Batch epoch of the safe head
    safe_epoch: Epoch,
    /// State struct to keep track of global state
    state: Arc<RwLock<State>>,
}

impl Driver<EngineApi, Pipeline> {
    pub fn from_config(config: Config) -> Result<Self> {
        let db = config
            .data_dir
            .as_ref()
            .map(Database::new)
            .unwrap_or_default();

        let head = db.read_head();

        let safe_head = head
            .as_ref()
            .map(|h| prev_block_id(&h.l2_block_info))
            .unwrap_or(config.chain.l2_genesis);

        let safe_epoch = head
            .map(|h| h.l1_epoch)
            .unwrap_or(config.chain.l1_start_epoch);

        tracing::info!("syncing from: {:?}", safe_head.hash);

        let config = Arc::new(config);
        let mut chain_watcher = ChainWatcher::new(safe_epoch.number, config.clone())?;
        let tx_recv = chain_watcher.take_tx_receiver().unwrap();

        let state = Arc::new(RwLock::new(State::new(
            safe_head,
            safe_epoch,
            chain_watcher,
        )));

        let engine = EngineApi::new(
            config.engine_api_url.clone().unwrap_or_default(),
            config.jwt_secret.clone(),
        );

        let pipeline = Pipeline::new(state.clone(), tx_recv, config)?;

        Ok(Self {
            db,
            engine,
            pipeline,
            safe_head,
            safe_epoch,
            state,
        })
    }
}

impl<E: L2EngineApi, P: Iterator<Item = PayloadAttributes>> Driver<E, P> {
    /// Creates a new Driver instance
    pub fn from_internals(engine: E, pipeline: P, config: Arc<Config>) -> Result<Self> {
        let safe_head = config.chain.l2_genesis;
        let safe_epoch = config.chain.l1_start_epoch;
        let chain_watcher = ChainWatcher::new(safe_epoch.number, config)?;
        let state = Arc::new(RwLock::new(State::new(
            safe_head,
            safe_epoch,
            chain_watcher,
        )));

        let db = Database::default();

        Ok(Self {
            pipeline,
            engine,
            db,
            safe_head,
            safe_epoch,
            state,
        })
    }

    /// Runs the Driver
    pub async fn start(&mut self) -> Result<()> {
        loop {
            self.advance().await?;
            tracing::info!(target: "magi", "head updated: {} {:?}", self.safe_head.number, self.safe_head.hash);
        }
    }

    /// Shuts down the driver
    pub async fn shutdown(&self) -> Result<()> {
        // TODO: flush the database
        Ok(())
    }

    /// Attempts to advance the execution node forward one block using derived
    /// L1 data. Errors if the most recent PayloadAttributes from the pipeline
    /// does not successfully advance the node
    pub async fn advance(&mut self) -> Result<()> {
        let next_attributes = loop {
            self.update_state();

            if let Some(next_attributes) = self.pipeline.next() {
                break next_attributes;
            }
        };

        tracing::debug!(target: "magi", "received new attributes from the pipeline");

        tracing::debug!("next attributes: {:?}", next_attributes);

        let new_epoch = *next_attributes.epoch.as_ref().unwrap();

        let payload = self.build_payload(next_attributes).await?;

        let new_head = BlockInfo {
            number: payload.block_number.as_u64(),
            hash: payload.block_hash,
            parent_hash: payload.parent_hash,
            timestamp: payload.timestamp.as_u64(),
        };

        self.push_payload(payload).await?;
        self.update_forkchoice(new_head, new_epoch).await?;

        Ok(())
    }

    fn update_state(&self) {
        let mut state = self.state.write().unwrap();
        state.update_l1_info();
        state.update_safe_head(self.safe_head, self.safe_epoch);
    }

    async fn build_payload(&self, attributes: PayloadAttributes) -> Result<ExecutionPayload> {
        let forkchoice = self.create_forkchoice_state();

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

    async fn update_forkchoice(&mut self, new_head: BlockInfo, new_epoch: Epoch) -> Result<()> {
        if self.safe_head != new_head {
            if self.safe_epoch != new_epoch {
                tracing::info!("saving new head to db: {:?}", new_head.hash);

                self.db.write_head(HeadInfo {
                    l2_block_info: new_head,
                    l1_epoch: new_epoch,
                })?;
            }

            self.safe_head = new_head;
            self.safe_epoch = new_epoch;
        }

        let forkchoice = self.create_forkchoice_state();
        let update = self.engine.forkchoice_updated(forkchoice, None).await?;

        if update.payload_status.status != Status::Valid {
            eyre::bail!(
                "could not accept new forkchoice: {:?}",
                update.payload_status.validation_error
            );
        }

        Ok(())
    }

    fn create_forkchoice_state(&self) -> ForkchoiceState {
        ForkchoiceState {
            head_block_hash: self.safe_head.hash,
            safe_block_hash: self.safe_head.hash,
            finalized_block_hash: H256::zero(),
        }
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
