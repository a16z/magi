use std::sync::Arc;

use ethers_core::types::H256;
use eyre::Result;

use crate::{
    backend::{Database, HeadInfo},
    common::BlockID,
    config::Config,
    derive::Pipeline,
    engine::{
        EngineApi, ExecutionPayload, ForkchoiceState, L2EngineApi, PayloadAttributes, Status,
    },
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
    pub safe_block: BlockID,
    /// Batch epoch of the safe head
    pub safe_epoch: u64,
    /// Most recent block hash that can be derived from finalized L1 data
    pub finalized_block: BlockID,
}

impl Driver<EngineApi, Pipeline> {
    pub fn from_config(config: Config) -> Result<Self> {
        let db = config
            .db_location
            .as_ref()
            .map(Database::new)
            .unwrap_or_default();

        let head = db.read_head();

        let safe_block = head
            .as_ref()
            .map(|h| prev_block_id(&h.l2_block_id))
            .unwrap_or(config.chain.l2_genesis);

        let safe_epoch = head
            .map(|h| h.l1_epoch_number)
            .unwrap_or(config.chain.l1_start_epoch.number);

        tracing::info!("syncing from: {:?}", safe_block.hash);

        let engine = EngineApi::new(
            config.engine_api_url.clone().unwrap_or_default(),
            config.jwt_secret.clone(),
        );
        let pipeline = Pipeline::new(safe_epoch, Arc::new(config))?;

        Ok(Self {
            db,
            engine,
            pipeline,
            safe_epoch,
            safe_block,
            finalized_block: BlockID::default(),
        })
    }
}

impl<E: L2EngineApi, P: Iterator<Item = PayloadAttributes>> Driver<E, P> {
    /// Creates a new Driver instance
    pub fn from_internals(engine: E, pipeline: P, config: Arc<Config>) -> Self {
        let safe_block = config.chain.l2_genesis;
        let safe_epoch = config.chain.l1_start_epoch.number;
        let finalized_block = BlockID::default();

        let db = Database::default();

        Self {
            pipeline,
            engine,
            db,
            safe_block,
            safe_epoch,
            finalized_block,
        }
    }

    /// Runs the Driver
    pub async fn start(&mut self) -> Result<()> {
        loop {
            self.advance().await?;
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
            if let Some(next_attributes) = self.pipeline.next() {
                break next_attributes;
            }
        };

        let new_epoch = next_attributes.epoch_number.unwrap();

        let payload = self.build_payload(next_attributes).await?;

        let new_block = BlockID {
            number: payload.block_number.as_u64(),
            hash: payload.block_hash,
            parent_hash: payload.parent_hash,
        };

        self.push_payload(payload).await?;
        self.update_forkchoice(new_block, new_epoch).await?;

        Ok(())
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
        if status.status != Status::Valid {
            eyre::bail!("invalid execution payload");
        }

        Ok(())
    }

    async fn update_forkchoice(&mut self, new_block: BlockID, new_epoch: u64) -> Result<()> {
        if self.safe_block != new_block {
            tracing::info!("chain head updated: {:?}", new_block.hash);
            if self.safe_epoch != new_epoch {
                tracing::info!("saving new head to db: {:?}", new_block.hash);

                self.db.write_head(HeadInfo {
                    l2_block_id: new_block,
                    l1_epoch_number: new_epoch,
                })?;
            }

            self.safe_block = new_block;
            self.safe_epoch = new_epoch;
        }

        let forkchoice = self.create_forkchoice_state();
        let update = self.engine.forkchoice_updated(forkchoice, None).await?;

        if update.payload_status.status != Status::Valid {
            eyre::bail!("could not accept new forkchoice");
        }

        Ok(())
    }

    fn create_forkchoice_state(&self) -> ForkchoiceState {
        ForkchoiceState {
            head_block_hash: self.safe_block.hash,
            safe_block_hash: self.safe_block.hash,
            finalized_block_hash: self.finalized_block.hash,
        }
    }
}

fn prev_block_id(block: &BlockID) -> BlockID {
    BlockID {
        number: block.number - 1,
        hash: block.parent_hash,
        parent_hash: H256::zero(),
    }
}
