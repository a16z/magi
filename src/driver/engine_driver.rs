use std::sync::Arc;

use ethers::providers::{Http, Middleware, Provider};
use ethers::types::Transaction;
use ethers::{
    types::{Block, H256},
    utils::keccak256,
};
use eyre::Result;
use tokio::time::{sleep, Duration};

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
    /// Most recent block found on the p2p network
    pub unsafe_head: BlockInfo,
    /// Batch epoch of the unsafe head (expected)
    pub unsafe_epoch: Epoch,
    /// Most recent block that can be derived from L1 data
    pub safe_head: BlockInfo,
    /// Batch epoch of the safe head
    pub safe_epoch: Epoch,
    /// Most recent block that can be derived from finalized L1 data
    pub finalized_head: BlockInfo,
    /// Batch epoch of the finalized head
    pub finalized_epoch: Epoch,
}

impl<E: Engine> EngineDriver<E> {
    pub async fn handle_attributes(
        &mut self,
        attributes: PayloadAttributes,
        update_safe: bool,
    ) -> Result<()> {
        let block: Option<Block<Transaction>> = self.block_at(attributes.timestamp.as_u64()).await;

        if let Some(block) = block {
            if should_skip(&block, &attributes)? {
                self.skip_attributes(attributes, block).await
            } else {
                self.unsafe_head = self.safe_head;
                self.process_attributes(attributes, update_safe).await
            }
        } else {
            self.process_attributes(attributes, update_safe).await
        }
    }

    pub async fn handle_unsafe_payload(&mut self, payload: &ExecutionPayload) -> Result<()> {
        self.push_payload(payload.clone()).await?;
        self.unsafe_head = payload.into();
        self.update_forkchoice().await?;

        tracing::info!(
            "head updated: {} {:?}",
            self.unsafe_head.number,
            self.unsafe_head.hash,
        );

        Ok(())
    }

    pub fn update_finalized(&mut self, head: BlockInfo, epoch: Epoch) {
        self.finalized_head = head;
        self.finalized_epoch = epoch;
    }

    pub fn reorg(&mut self) {
        self.unsafe_head = self.finalized_head;
        self.unsafe_epoch = self.finalized_epoch;
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

    async fn process_attributes(
        &mut self,
        attributes: PayloadAttributes,
        update_safe: bool,
    ) -> Result<()> {
        let new_epoch = *attributes.epoch.as_ref().unwrap();

        let payload = self.build_payload(attributes).await?;

        let new_head = BlockInfo {
            number: payload.block_number.as_u64(),
            hash: payload.block_hash,
            parent_hash: payload.parent_hash,
            timestamp: payload.timestamp.as_u64(),
        };

        self.push_payload(payload).await?;
        if update_safe {
            self.update_safe_head(new_head, new_epoch, true)?;
        } else {
            self.unsafe_head = new_head;
            self.unsafe_epoch = new_epoch;
        }
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
        self.update_safe_head(new_head, new_epoch, false)?;
        self.update_forkchoice().await?;

        Ok(())
    }

    async fn build_payload(&self, attributes: PayloadAttributes) -> Result<ExecutionPayload> {
        let forkchoice = self.create_forkchoice_state();
        let no_tx_pool = attributes.no_tx_pool;

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

        if !no_tx_pool {
            // Wait before fetching the payload to give the engine time to build it.
            sleep(Duration::from_secs(self.blocktime)).await
        }
        self.engine.get_payload(id).await
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
        reorg_unsafe: bool,
    ) -> Result<()> {
        if self.safe_head != new_head {
            self.safe_head = new_head;
            self.safe_epoch = new_epoch;
        }

        if reorg_unsafe || self.safe_head.number > self.unsafe_head.number {
            self.unsafe_head = new_head;
        }

        Ok(())
    }

    fn create_forkchoice_state(&self) -> ForkchoiceState {
        ForkchoiceState {
            head_block_hash: self.unsafe_head.hash,
            safe_block_hash: self.safe_head.hash,
            finalized_block_hash: self.finalized_head.hash,
        }
    }

    async fn block_at(&self, timestamp: u64) -> Option<Block<Transaction>> {
        let time_diff = timestamp as i64 - self.finalized_head.timestamp as i64;
        let blocks = time_diff / self.blocktime as i64;
        let block_num = self.finalized_head.number as i64 + blocks;
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
        finalized_head: BlockInfo,
        finalized_epoch: Epoch,
        provider: Provider<Http>,
        config: &Arc<Config>,
    ) -> Result<Self> {
        let engine = Arc::new(EngineApi::new(&config.l2_engine_url, &config.jwt_secret));

        Ok(Self {
            engine,
            provider,
            blocktime: config.chain.blocktime,
            unsafe_head: finalized_head,
            unsafe_epoch: finalized_epoch,
            safe_head: finalized_head,
            safe_epoch: finalized_epoch,
            finalized_head,
            finalized_epoch,
        })
    }
}
