//! A module to handle block production & validation

use std::sync::Arc;

use alloy_primitives::keccak256;
use alloy_rpc_types::{BlockTransactions, Block};
use alloy_provider::{Provider, ReqwestProvider};
use eyre::Result;

use crate::{
    common::{BlockInfo, Epoch},
    config::Config,
    engine::{Engine, EngineApi, ExecutionPayload, ForkchoiceState, PayloadAttributes, Status},
};

/// The EngineDriver is responsible for initiating block production & validation via the [EngineApi]
pub struct EngineDriver<E: Engine> {
    /// The L2 execution engine
    engine: Arc<E>,
    /// Provider for the local L2 execution RPC
    provider: ReqwestProvider,
    /// Blocktime of the L2 chain
    blocktime: u64,
    /// Most recent block found on the p2p network
    pub unsafe_head: BlockInfo,
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
    /// Initiates validation & production of a new L2 block from the given [PayloadAttributes] and updates the forkchoice
    pub async fn handle_attributes(&mut self, attributes: PayloadAttributes) -> Result<()> {
        let timestamp: u64 = attributes.timestamp.try_into()?;
        let block: Option<Block> = self.block_at(timestamp).await;

        if let Some(block) = block {
            if should_skip(&block, &attributes)? {
                self.skip_attributes(attributes, block).await
            } else {
                self.unsafe_head = self.safe_head;
                self.process_attributes(attributes).await
            }
        } else {
            self.process_attributes(attributes).await
        }
    }

    /// Instructs the engine to create a block and updates the forkchoice, based on a payload received via p2p gossip.
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

    /// Updates the [EngineDriver] finalized head & epoch
    pub fn update_finalized(&mut self, head: BlockInfo, epoch: Epoch) {
        self.finalized_head = head;
        self.finalized_epoch = epoch;
    }

    /// Sets the [EngineDriver] unsafe & safe heads, and safe epoch to the current finalized head & epoch.
    pub fn reorg(&mut self) {
        self.unsafe_head = self.finalized_head;
        self.safe_head = self.finalized_head;
        self.safe_epoch = self.finalized_epoch;
    }

    /// Sends a `ForkchoiceUpdated` message to check if the [Engine] is ready.
    pub async fn engine_ready(&self) -> bool {
        let forkchoice = self.create_forkchoice_state();
        self.engine
            .forkchoice_updated(forkchoice, None)
            .await
            .is_ok()
    }

    /// Initiates validation & production of a new block:
    /// - Sends the [PayloadAttributes] to the engine via `engine_forkchoiceUpdatedV2` (V3 post Ecotone) and retrieves the [ExecutionPayload]
    /// - Executes the [ExecutionPayload] to create a block via `engine_newPayloadV2` (V3 post Ecotone)
    /// - Updates the [EngineDriver] `safe_head`, `safe_epoch`, and `unsafe_head`
    /// - Updates the forkchoice and sends this to the engine via `engine_forkchoiceUpdatedV2` (v3 post Ecotone)
    async fn process_attributes(&mut self, attributes: PayloadAttributes) -> Result<()> {
        let new_epoch = *attributes.epoch.as_ref().unwrap();

        let payload = self.build_payload(attributes).await?;

        let new_head = BlockInfo {
            number: payload.block_number.try_into().unwrap_or_default(),
            hash: payload.block_hash,
            parent_hash: payload.parent_hash,
            timestamp: payload.timestamp.try_into().unwrap_or_default(),
        };

        self.push_payload(payload).await?;
        self.update_safe_head(new_head, new_epoch, true)?;
        self.update_forkchoice().await?;

        Ok(())
    }

    /// Updates the forkchoice by sending `engine_forkchoiceUpdatedV2` (v3 post Ecotone) to the engine with no payload.
    async fn skip_attributes(
        &mut self,
        attributes: PayloadAttributes,
        block: Block,
    ) -> Result<()> {
        let new_epoch = *attributes.epoch.as_ref().unwrap();
        let new_head = BlockInfo::try_from(block)?;
        self.update_safe_head(new_head, new_epoch, false)?;
        self.update_forkchoice().await?;

        Ok(())
    }

    /// Sends [PayloadAttributes] via a `ForkChoiceUpdated` message to the [Engine] and returns the [ExecutionPayload] sent by the Execution Client.
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

    /// Sends the given [ExecutionPayload] to the [Engine] via `NewPayload`
    async fn push_payload(&self, payload: ExecutionPayload) -> Result<()> {
        let status = self.engine.new_payload(payload).await?;
        if status.status != Status::Valid && status.status != Status::Accepted {
            eyre::bail!("invalid execution payload");
        }

        Ok(())
    }

    /// Sends a `ForkChoiceUpdated` message to the [Engine] with the current `Forkchoice State` and no payload.
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

    /// Updates the current `safe_head` & `safe_epoch`.
    ///
    /// Also updates the current `unsafe_head` to the given `new_head` if `reorg_unsafe` is `true`, or if the updated `safe_head` is newer than the current `unsafe_head`
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

    /// Creates a [ForkchoiceState]:
    /// - `head_block` = `unsafe_head`
    /// - `safe_block` = `safe_head`
    /// - `finalized_block` = `finalized_head`
    fn create_forkchoice_state(&self) -> ForkchoiceState {
        ForkchoiceState {
            head_block_hash: self.unsafe_head.hash,
            safe_block_hash: self.safe_head.hash,
            finalized_block_hash: self.finalized_head.hash,
        }
    }

    /// Fetches the L2 block for a given timestamp from the L2 Execution Client
    async fn block_at(&self, timestamp: u64) -> Option<Block> {
        let time_diff = timestamp as i64 - self.finalized_head.timestamp as i64;
        let blocks = time_diff / self.blocktime as i64;
        let block_num = self.finalized_head.number as i64 + blocks;
        self.provider
            .get_block((block_num as u64).into(), true)
            .await
            .ok()?
    }
}

/// True if transactions in [PayloadAttributes] are not the same as those in a fetched L2 [Block]
fn should_skip(block: &Block, attributes: &PayloadAttributes) -> Result<bool> {
    tracing::debug!(
        "comparing block at {} with attributes at {}",
        block.header.timestamp,
        attributes.timestamp
    );

    let attributes_hashes = attributes
        .transactions
        .as_ref()
        .unwrap()
        .iter()
        .map(|tx| keccak256(&tx.0))
        .collect::<Vec<_>>();

    let BlockTransactions::Full(txs) = &block.transactions else {
        return Ok(true);
    };

    let block_hashes = txs
        .iter()
        .map(|tx| tx.hash())
        .collect::<Vec<_>>();

    tracing::debug!("attribute hashes: {:?}", attributes_hashes);
    tracing::debug!("block hashes: {:?}", block_hashes);

    let is_same = attributes_hashes == block_hashes
        && attributes.timestamp == block.header.timestamp
        && block.header.mix_hash.map_or(false, |m| m == attributes.prev_randao)
        && attributes.suggested_fee_recipient
            == block.header.miner
        && attributes.gas_limit == block.header.gas_limit;

    Ok(is_same)
}

impl EngineDriver<EngineApi> {
    /// Creates a new [EngineDriver] and builds the [EngineApi] client
    pub fn new(
        finalized_head: BlockInfo,
        finalized_epoch: Epoch,
        provider: ReqwestProvider,
        config: &Arc<Config>,
    ) -> Result<Self> {
        let engine = Arc::new(EngineApi::new(&config.l2_engine_url, &config.jwt_secret));

        Ok(Self {
            engine,
            provider,
            blocktime: config.chain.blocktime,
            unsafe_head: finalized_head,
            safe_head: finalized_head,
            safe_epoch: finalized_epoch,
            finalized_head,
            finalized_epoch,
        })
    }
}
