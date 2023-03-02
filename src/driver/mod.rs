use std::sync::Arc;

use ethers_core::types::H256;
use eyre::Result;

use crate::{
    config::Config,
    engine::{ExecutionPayload, ForkchoiceState, L2EngineApi, PayloadAttributes, Status},
};

/// Driver is responsible for advancing the execution node by feeding
/// the derived chain into the engine API
pub struct Driver<E: L2EngineApi, P: Iterator<Item = PayloadAttributes>> {
    /// The derivation pipeline
    pipeline: P,
    /// The L2 execution engine
    engine: E,
    /// Most recent block hash. Not necessarily derived from L1 data
    pub head_block_hash: H256,
    /// Most recent block hash that can be derived from L1 data
    pub safe_block_hash: H256,
    /// Most recent block hash that can be derived from finalized L1 data
    pub finalized_hash: H256,
}

impl<E: L2EngineApi, P: Iterator<Item = PayloadAttributes>> Driver<E, P> {
    /// Creates a new Driver instance
    pub fn new(engine: E, pipeline: P, config: Arc<Config>) -> Self {
        let head_block_hash = config.chain.l2_genesis.hash;
        let safe_block_hash = config.chain.l2_genesis.hash;
        let finalized_hash = H256::zero();

        Self {
            pipeline,
            engine,
            head_block_hash,
            safe_block_hash,
            finalized_hash,
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

        let payload = self.build_payload(next_attributes).await?;
        let new_hash = payload.block_hash;

        self.push_payload(payload).await?;
        self.update_forkchoice(new_hash).await?;

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

    async fn update_forkchoice(&mut self, new_hash: H256) -> Result<()> {
        if self.head_block_hash != new_hash {
            tracing::info!("chain head updated: {:?}", new_hash);
            self.head_block_hash = new_hash;
            self.safe_block_hash = new_hash;
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
            head_block_hash: self.head_block_hash,
            safe_block_hash: self.safe_block_hash,
            finalized_block_hash: self.finalized_hash,
        }
    }
}
