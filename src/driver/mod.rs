use std::sync::Arc;

use ethers_core::types::H256;
use eyre::Result;

use crate::{
    config::Config,
    derive::Pipeline,
    engine::{ForkchoiceState, L2EngineApi, PayloadAttributes, Status},
};

pub struct Driver<E: L2EngineApi> {
    pipeline: Pipeline,
    engine: E,
    head_block_hash: H256,
    safe_block_hash: H256,
    finalized_hash: H256,
}

impl<E: L2EngineApi> Driver<E> {
    pub fn new(engine: E, config: Config) -> Result<Self> {
        let head_block_hash = config.chain.l2_genesis.hash;
        let safe_block_hash = config.chain.l2_genesis.hash;
        let finalized_hash = H256::zero();

        let config = Arc::new(config);
        let epoch_start = config.chain.l1_start_epoch.number;

        let pipeline = Pipeline::new(epoch_start, config)?;

        Ok(Self {
            pipeline,
            engine,
            head_block_hash,
            safe_block_hash,
            finalized_hash,
        })
    }

    pub async fn run(&mut self) {
        loop {
            if let Some(next_attributes) = self.pipeline.next() {
                if let Err(err) = self.advance_engine(next_attributes).await {
                    tracing::warn!("driver error: {}", err);
                }
            }
        }
    }

    async fn advance_engine(&mut self, attributes: PayloadAttributes) -> Result<()> {
        let forkchoice = self.create_forkchoice_state();

        // build payload
        let update = self
            .engine
            .forkchoice_updated(forkchoice, Some(attributes))
            .await?;

        if update.payload_status.status != Status::Valid {
            eyre::bail!("invalid payload attributes");
        }

        // fetch new payload
        let id = update
            .payload_id
            .ok_or(eyre::eyre!("engine did not return payload id"))?;

        let payload = self.engine.get_payload(id).await?;
        let new_hash = payload.block_hash;

        // push new payload
        let status = self.engine.new_payload(payload).await?;
        if status.status != Status::Accepted {
            eyre::bail!("invalid execution payload");
        }

        // update internal hashes
        self.head_block_hash = new_hash;
        self.safe_block_hash = new_hash;

        // update forkchoice
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
