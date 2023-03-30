use std::sync::Arc;

use ethers_core::{
    types::{Block, H256},
    utils::keccak256,
};
use ethers_providers::{Http, Middleware, Provider};
use eyre::Result;
use tokio::spawn;

use crate::{
    common::{BlockInfo, Epoch},
    config::Config,
    engine::{Engine, EngineApi, ExecutionPayload, ForkchoiceState, PayloadAttributes, Status},
};

pub struct EngineDriver<E: Engine> {
    /// The L2 execution engine
    engine: Arc<E>,
    /// Provider for the local L2 execution RPC
    provider: Provider<Http>,
    /// Most recent block hash that can be derived from L1 data
    pub safe_head: BlockInfo,
    /// Batch epoch of the safe head
    pub safe_epoch: Epoch,
    /// Most recent block hash that can be derived from finalized L1 data
    pub finalized_head: BlockInfo,
    /// Batch epoch of the finalized head
    pub finalized_epoch: Epoch,
}

impl<E: Engine> EngineDriver<E> {
    pub async fn handle_attributes(&mut self, attributes: PayloadAttributes) -> Result<()> {
        let block: Option<Block<H256>> = self.provider.get_block(self.safe_head.number + 1).await?;

        if let Some(block) = block {
            if should_skip(&block, &attributes)? {
                self.skip_attributes(attributes, block)
            } else {
                self.process_attributes(attributes).await
            }
        } else {
            self.process_attributes(attributes).await
        }
    }

    pub fn update_finalized(&mut self, head: BlockInfo, epoch: Epoch) {
        self.finalized_head = head;
        self.finalized_epoch = epoch;
    }

    pub fn reorg(&mut self) {
        self.safe_head = self.finalized_head;
        self.safe_epoch = self.finalized_epoch;
    }

    pub async fn engine_ready(&self) -> bool {
        let forkchoice = create_forkchoice_state(self.safe_head.hash, self.finalized_head.hash);
        self.engine.forkchoice_updated(forkchoice, None).await.is_ok()
    }

    async fn process_attributes(&mut self, attributes: PayloadAttributes) -> Result<()> {
        let new_epoch = *attributes.epoch.as_ref().unwrap();

        let payload = self.build_payload(attributes).await?;

        let new_head = BlockInfo {
            number: payload.block_number.as_u64(),
            hash: payload.block_hash,
            parent_hash: payload.parent_hash,
            timestamp: payload.timestamp.as_u64(),
        };

        self.push_payload(payload).await?;
        self.update_forkchoice(new_head);

        self.update_safe_head(new_head, new_epoch)?;

        Ok(())
    }

    fn skip_attributes(&mut self, attributes: PayloadAttributes, block: Block<H256>) -> Result<()> {
        let new_epoch = *attributes.epoch.as_ref().unwrap();
        let new_head = BlockInfo::try_from(block)?;
        self.update_safe_head(new_head, new_epoch)?;

        Ok(())
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

    fn update_safe_head(&mut self, new_head: BlockInfo, new_epoch: Epoch) -> Result<()> {
        if self.safe_head != new_head {
            self.safe_head = new_head;
            self.safe_epoch = new_epoch;
        }

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

fn should_skip(block: &Block<H256>, attributes: &PayloadAttributes) -> Result<bool> {
    let attributes_hashes = attributes
        .transactions
        .as_ref()
        .unwrap()
        .iter()
        .map(|tx| H256(keccak256(&tx.0)))
        .collect::<Vec<_>>();

    let is_same = attributes_hashes == block.transactions
        && attributes.timestamp.as_u64() == block.timestamp.as_u64()
        && attributes.prev_randao == block.mix_hash.unwrap()
        && attributes.suggested_fee_recipient == block.author.unwrap()
        && attributes.gas_limit.as_u64() == block.gas_limit.as_u64();

    Ok(is_same)
}

impl EngineDriver<EngineApi> {
    pub fn new(
        finalized_head: BlockInfo,
        finalized_epoch: Epoch,
        config: &Arc<Config>,
    ) -> Result<Self> {
        let engine = Arc::new(EngineApi::new(
            config.l2_engine_url.clone().unwrap_or_default(),
            config.jwt_secret.clone(),
        ));

        let provider = Provider::try_from(config.l2_rpc_url.clone().unwrap())?;

        Ok(Self {
            engine,
            provider,
            safe_head: finalized_head,
            safe_epoch: finalized_epoch,
            finalized_head,
            finalized_epoch,
        })
    }
}
