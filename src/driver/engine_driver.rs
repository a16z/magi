use std::sync::Arc;

use ethers::providers::{Http, Middleware, Provider};
use ethers::{
    types::{Block, H256},
    utils::keccak256,
};
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
    /// Blocktime of the L2 chain
    blocktime: u64,
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
        let block: Option<Block<H256>> = self.block_at(attributes.timestamp.as_u64()).await;

        if let Some(block) = block {
            if should_skip(&block, &attributes)? {
                tracing::info!("skipping block");
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
        let forkchoice = self.create_forkchoice_state();
        self.engine
            .forkchoice_updated(forkchoice, None)
            .await
            .is_ok()
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
        self.update_safe_head(new_head, new_epoch)?;
        self.update_forkchoice();

        Ok(())
    }

    fn skip_attributes(&mut self, attributes: PayloadAttributes, block: Block<H256>) -> Result<()> {
        let new_epoch = *attributes.epoch.as_ref().unwrap();
        let new_head = BlockInfo::try_from(block)?;
        self.update_safe_head(new_head, new_epoch)?;

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
        if status.status != Status::Valid && status.status != Status::Accepted {
            eyre::bail!("invalid execution payload");
        }

        Ok(())
    }

    fn update_forkchoice(&self) {
        let forkchoice = self.create_forkchoice_state();
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

    fn create_forkchoice_state(&self) -> ForkchoiceState {
        ForkchoiceState {
            head_block_hash: self.safe_head.hash,
            safe_block_hash: self.safe_head.hash,
            finalized_block_hash: self.finalized_head.hash,
        }
    }

    async fn block_at(&self, timestamp: u64) -> Option<Block<H256>> {
        let time_diff = timestamp as i64 - self.finalized_head.timestamp as i64;
        let blocks = time_diff / self.blocktime as i64;
        let block_num = self.finalized_head.number as i64 + blocks;
        self.provider.get_block(block_num as u64).await.ok()?
    }
}

fn should_skip(block: &Block<H256>, attributes: &PayloadAttributes) -> Result<bool> {
    tracing::debug!(
        "comparing block at {} with attributes at {}",
        block.timestamp,
        attributes.timestamp
    );

    tracing::debug!("block: {:?}", block);
    tracing::debug!("attributes: {:?}", attributes);

    let attributes_hashes = attributes
        .transactions
        .as_ref()
        .unwrap()
        .iter()
        .map(|tx| H256(keccak256(&tx.0)))
        .collect::<Vec<_>>();

    tracing::debug!("attribute hashes: {:?}", attributes_hashes);

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
        let engine = Arc::new(EngineApi::new(&config.l2_engine_url, &config.jwt_secret));
        let provider = Provider::try_from(&config.l2_rpc_url)?;

        Ok(Self {
            engine,
            provider,
            blocktime: config.chain.blocktime,
            safe_head: finalized_head,
            safe_epoch: finalized_epoch,
            finalized_head,
            finalized_epoch,
        })
    }
}
