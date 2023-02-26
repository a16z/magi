use std::sync::Arc;

use ethers_core::types::H256;

use crate::{config::Config, derive::Pipeline, engine::{PayloadAttributes, L2EngineApi, ForkchoiceState}};

pub struct Driver<E: L2EngineApi> {
    pipeline: Pipeline,
    engine: E,
    head_block_hash: H256,
    safe_block_hash: H256,
    finalized_hash: H256,
}

impl<E: L2EngineApi> Driver<E> {
    pub fn new(engine: E, config: Config) -> Self {
        let head_block_hash = config.chain.l2_genesis.hash;
        let safe_block_hash = config.chain.l2_genesis.hash;
        let finalized_hash = config.chain.l2_genesis.hash;

        let config = Arc::new(config);
        let epoch_start = config.chain.l1_start_epoch.number;

        let pipeline = Pipeline::new(epoch_start, config);

        Self { pipeline, engine, head_block_hash, safe_block_hash, finalized_hash }
    }

    pub async fn run(&mut self) {
        loop {
            if let Some(next_attributes) = self.pipeline.next() {
                self.advance_engine(next_attributes).await
            }
        }
    }

    async fn advance_engine(&mut self, attributes: PayloadAttributes) {
        let forkchoice = self.create_forkchoice_state();

        // build payload
        let update = self.engine.forkchoice_updated(forkchoice, Some(attributes)).await.unwrap();
        let id = update.payload_id.unwrap();

        // fetch new payload
        let payload = self.engine.get_payload(id).await.unwrap();
        let new_hash = payload.block_hash;

        // push new payload
        let _status = self.engine.new_payload(payload).await.unwrap();

        // update internal hashes
        self.head_block_hash = new_hash;
        self.safe_block_hash = new_hash;
        self.finalized_hash = new_hash;

        // update forkchoice
        let forkchoice = self.create_forkchoice_state();
        let _update = self.engine.forkchoice_updated(forkchoice, None).await.unwrap();
    }

    fn create_forkchoice_state(&self) -> ForkchoiceState {
        ForkchoiceState {
            head_block_hash: self.head_block_hash,
            safe_block_hash: self.safe_block_hash,
            finalized_block_hash: self.finalized_hash,
        }
    }
}
