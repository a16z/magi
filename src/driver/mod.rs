use ethers_core::types::H256;
use eyre::Result;

use crate::{
    config::Config,
    engine::{ExecutionPayload, ForkchoiceState, L2EngineApi, PayloadAttributes, Status},
};

pub struct Driver<E: L2EngineApi, P: Iterator<Item = PayloadAttributes>> {
    pipeline: P,
    engine: E,
    head_block_hash: H256,
    safe_block_hash: H256,
    finalized_hash: H256,
}

impl<E: L2EngineApi, P: Iterator<Item = PayloadAttributes>> Driver<E, P> {
    pub fn new(engine: E, pipeline: P, config: Config) -> Result<Self> {
        let head_block_hash = config.chain.l2_genesis.hash;
        let safe_block_hash = config.chain.l2_genesis.hash;
        let finalized_hash = H256::zero();

        Ok(Self {
            pipeline,
            engine,
            head_block_hash,
            safe_block_hash,
            finalized_hash,
        })
    }

    pub async fn advance(&mut self) -> Result<()> {
        if let Some(next_attributes) = self.pipeline.next() {
            self.advance_engine(next_attributes).await?;
        }

        Ok(())
    }

    async fn advance_engine(&mut self, attributes: PayloadAttributes) -> Result<()> {
        let payload = self.build_payload(attributes).await?;
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
        if status.status != Status::Accepted {
            eyre::bail!("invalid execution payload");
        }

        Ok(())
    }

    async fn update_forkchoice(&mut self, new_hash: H256) -> Result<()> {
        self.head_block_hash = new_hash;
        self.safe_block_hash = new_hash;

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
