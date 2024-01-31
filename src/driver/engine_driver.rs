use std::sync::Arc;

use ethers::providers::{Http, Middleware, Provider};
use ethers::types::Transaction;
use ethers::{
    types::{Block, H256},
    utils::keccak256,
};
use eyre::Result;

use crate::{
    config::Config,
    engine::{
        Engine, EngineApi, ExecutionPayload, ForkchoiceState, PayloadAttributes, PayloadId, Status,
    },
    types::common::{BlockInfo, Epoch, HeadInfo},
};

pub struct EngineDriver<E: Engine> {
    /// The L2 execution engine
    engine: Arc<E>,
    /// Provider for the local L2 execution RPC
    provider: Provider<Http>,
    /// Blocktime of the L2 chain
    blocktime: u64,

    /// The unsafe head info of the L2: blocks from P2P or sequencer.
    pub unsafe_info: HeadInfo,
    /// The safe head info of the L2: when referenced L1 block which are not finalized yet.
    pub safe_info: HeadInfo,
    /// The finalized head info of the L2: when referenced L1 block can't be reverted already.
    pub finalized_info: HeadInfo,

    /// Engine sync head info.
    pub sync_info: HeadInfo,
}

impl<E: Engine> EngineDriver<E> {
    pub async fn handle_attributes(&mut self, attributes: PayloadAttributes) -> Result<()> {
        let block: Option<Block<Transaction>> = self.block_at(attributes.timestamp.as_u64()).await;

        if let Some(block) = block {
            if should_skip(&block, &attributes)? {
                self.skip_attributes(attributes, block).await
            } else {
                self.unsafe_info = self.safe_info;
                self.sync_info = self.safe_info;

                self.process_attributes(attributes).await
            }
        } else {
            self.process_attributes(attributes).await
        }
    }

    pub async fn handle_unsafe_payload(&mut self, payload: &ExecutionPayload) -> Result<()> {
        self.push_payload(payload.clone()).await?;

        self.unsafe_info = payload.try_into()?;
        self.sync_info = self.unsafe_info;

        self.update_forkchoice().await?;

        tracing::info!(
            "head updated: {} {:?}",
            self.unsafe_info.head.number,
            self.unsafe_info.head.hash,
        );

        Ok(())
    }

    pub fn update_finalized(&mut self, head: BlockInfo, epoch: Epoch, seq_number: u64) {
        self.finalized_info = HeadInfo::new(head, epoch, seq_number)
    }

    pub fn reorg(&mut self) {
        self.unsafe_info = self.finalized_info;
        self.safe_info = self.finalized_info;
        self.sync_info = self.finalized_info;
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
        let seq_number = attributes.seq_number.unwrap();

        let payload_id = self.start_payload_building(attributes).await?;
        let payload = self.get_payload(payload_id).await?;

        let new_head = BlockInfo {
            number: payload.block_number.as_u64(),
            hash: payload.block_hash,
            parent_hash: payload.parent_hash,
            timestamp: payload.timestamp.as_u64(),
        };

        self.push_payload(payload).await?;
        self.update_safe_head(new_head, new_epoch, seq_number, true)?;
        self.update_forkchoice().await?;

        Ok(())
    }

    async fn skip_attributes(
        &mut self,
        attributes: PayloadAttributes,
        block: Block<Transaction>,
    ) -> Result<()> {
        let new_epoch = *attributes.epoch.as_ref().unwrap();
        let new_head = BlockInfo::try_from(block)?;

        self.update_safe_head(new_head, new_epoch, attributes.seq_number.unwrap(), false)?;
        self.update_forkchoice().await?;

        Ok(())
    }

    pub async fn start_payload_building(&self, attributes: PayloadAttributes) -> Result<PayloadId> {
        let forkchoice = self.create_forkchoice_state();

        let update = self
            .engine
            .forkchoice_updated(forkchoice, Some(attributes))
            .await?;

        if update.payload_status.status != Status::Valid {
            eyre::bail!("invalid payload attributes");
        }

        update
            .payload_id
            .ok_or(eyre::eyre!("engine did not return payload id"))
    }

    pub async fn get_payload(&self, payload_id: PayloadId) -> Result<ExecutionPayload> {
        self.engine.get_payload(payload_id).await
    }

    async fn push_payload(&self, payload: ExecutionPayload) -> Result<()> {
        let status = self.engine.new_payload(payload).await?;
        if status.status != Status::Valid && status.status != Status::Accepted {
            eyre::bail!("invalid execution payload");
        }

        Ok(())
    }

    async fn update_forkchoice(&self) -> Result<()> {
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

    fn update_safe_head(
        &mut self,
        new_head: BlockInfo,
        new_epoch: Epoch,
        new_seq_number: u64,
        reorg_unsafe: bool,
    ) -> Result<()> {
        if self.safe_info.head != new_head {
            self.safe_info = HeadInfo::new(new_head, new_epoch, new_seq_number);
            self.sync_info = self.safe_info;
        }

        if reorg_unsafe || self.safe_info.head.number > self.unsafe_info.head.number {
            self.unsafe_info = HeadInfo::new(new_head, new_epoch, new_seq_number);
            self.sync_info = self.unsafe_info;
        }

        Ok(())
    }

    fn create_forkchoice_state(&self) -> ForkchoiceState {
        ForkchoiceState {
            head_block_hash: self.unsafe_info.head.hash,
            safe_block_hash: self.safe_info.head.hash,
            finalized_block_hash: self.finalized_info.head.hash,
        }
    }

    async fn block_at(&self, timestamp: u64) -> Option<Block<Transaction>> {
        let time_diff = timestamp as i64 - self.finalized_info.head.timestamp as i64;
        let blocks = time_diff / self.blocktime as i64;
        let block_num = self.finalized_info.head.number as i64 + blocks;
        self.provider
            .get_block_with_txs(block_num as u64)
            .await
            .ok()?
    }
}

fn should_skip(block: &Block<Transaction>, attributes: &PayloadAttributes) -> Result<bool> {
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

    let block_hashes = block
        .transactions
        .iter()
        .map(|tx| tx.hash())
        .collect::<Vec<_>>();

    tracing::debug!("attribute hashes: {:?}", attributes_hashes);

    let is_same = attributes_hashes == block_hashes
        && attributes.timestamp.as_u64() == block.timestamp.as_u64()
        && attributes.prev_randao == block.mix_hash.unwrap()
        && attributes.suggested_fee_recipient == block.author.unwrap()
        && attributes.gas_limit.as_u64() == block.gas_limit.as_u64();

    Ok(is_same)
}

impl EngineDriver<EngineApi> {
    pub fn new(
        finalized_info: HeadInfo,
        safe_info: HeadInfo,
        unsafe_info: HeadInfo,
        provider: Provider<Http>,
        config: &Arc<Config>,
    ) -> Result<Self> {
        let engine = Arc::new(EngineApi::new(&config.l2_engine_url, &config.jwt_secret));

        Ok(Self {
            engine,
            provider,
            blocktime: config.chain.block_time,
            finalized_info,
            safe_info,
            unsafe_info,
            sync_info: unsafe_info,
        })
    }
}
