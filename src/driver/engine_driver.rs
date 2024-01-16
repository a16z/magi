use ethers::providers::{Http, Middleware, Provider};
use ethers::types::{Transaction, U64};
use ethers::{
    types::{Block, H256},
    utils::keccak256,
};
use eyre::Result;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};

use crate::{
    common::{BlockInfo, Epoch},
    config::Config,
    engine::{Engine, EngineApi, ExecutionPayload, ForkchoiceState, PayloadAttributes, Status},
};

use super::HeadInfo;

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

#[derive(Debug)]
pub enum Action {
    /// Indicates that the attributes should be skipped because
    /// a corresponding block (with info `BlockInfo`) already exists.
    /// The head should be updated to the given block.
    Skip(BlockInfo),
    /// Indicates that the attributes should be processed.
    /// If `bool` is true, reset unsafe head prior to processing.
    Process(bool),
}

pub enum ChainHeadType {
    /// The safe head.
    Safe,
    /// The unsafe head. Contains the expected head.
    Unsafe(BlockInfo),
}

#[derive(Debug, Error)]
pub enum EngineDriverError {
    #[error("mismatching unsafe head (expected: {1}, actual: {2}) -- context: {0}")]
    UnsafeHeadMismatch(String, H256, H256),
    #[error(transparent)]
    Other(#[from] eyre::Error),
}

/// Handles the given attributes.
/// Functionally equivalent to [EngineDriver<E>::handle_attributes], but manages
/// the engine driver RW lock by acquiring the write lock only as necessary.
pub async fn handle_attributes<E: Engine>(
    attrs: PayloadAttributes,
    target: &ChainHeadType,
    engine_driver: Arc<RwLock<EngineDriver<E>>>,
) -> Result<(), EngineDriverError> {
    let action = {
        let engine_driver = engine_driver.read().await;
        engine_driver.determine_action(&attrs).await?
    };
    execute_action(attrs, action, target, engine_driver).await
}

/// Executes `action` for `attrs` at the `target` (either the safe or unsafe head).
/// An error is returned if the action fails.
pub async fn execute_action<E: Engine>(
    attrs: PayloadAttributes,
    action: Action,
    target: &ChainHeadType,
    engine_driver: Arc<RwLock<EngineDriver<E>>>,
) -> Result<(), EngineDriverError> {
    match action {
        // Skip processing the attributes (fork-choice update-only).
        Action::Skip(info) => {
            let mut engine_driver = engine_driver.write().await;
            let epoch = *attrs.epoch.as_ref().unwrap();
            match target {
                ChainHeadType::Safe => engine_driver.update_safe_head(info, epoch, false),
                ChainHeadType::Unsafe(_) => engine_driver.update_unsafe_head(info, epoch),
            }
        }
        // Process the attributes (build payload + fork-choice update).
        Action::Process(reorg_unsafe) => {
            if reorg_unsafe {
                let mut engine_driver = engine_driver.write().await;
                match target {
                    ChainHeadType::Safe => {
                        let safe_head = engine_driver.safe_head;
                        let safe_epoch = engine_driver.safe_epoch;
                        engine_driver.update_unsafe_head(safe_head, safe_epoch);
                    }
                    // TODO: support this (potentially required for restart-related edge-cases).
                    ChainHeadType::Unsafe(_) => {
                        let err = eyre::eyre!("not supported");
                        return Err(EngineDriverError::Other(err));
                    }
                }
            }
            // Build new payload.
            let (new_head, new_epoch) = build_payload(attrs, target, engine_driver.clone()).await?;
            // Book-keeping: prepare for next fork-choice update by updating the head.
            {
                let mut engine_driver = engine_driver.write().await;
                match target {
                    ChainHeadType::Safe => {
                        engine_driver.update_safe_head(new_head, new_epoch, true)
                    }
                    ChainHeadType::Unsafe(_) => {
                        validate_head_consistency(
                            &engine_driver,
                            target,
                            "update_unsafe".to_string(),
                        )?;
                        engine_driver.update_unsafe_head(new_head, new_epoch)
                    }
                }
            }
            let engine_driver = engine_driver.read().await;
            let target = ChainHeadType::Unsafe(new_head);
            // Validate chain head consistency (probably non-essential, but avoids unnecessary work).
            validate_head_consistency(&engine_driver, &target, "update_fc".to_string())?;
            // Final fork-choice update.
            engine_driver.update_forkchoice().await?;
        }
    }
    Ok(())
}

/// Builds a payload using the given attributes. Returns the built head and epoch.
/// If `attrs.no_tx_pool` is false, it will wait for the blocktime to pass before finalizing the payload.
async fn build_payload<E: Engine>(
    attrs: PayloadAttributes,
    target: &ChainHeadType,
    engine_driver: Arc<RwLock<EngineDriver<E>>>,
) -> Result<(BlockInfo, Epoch), EngineDriverError> {
    let no_tx_pool = attrs.no_tx_pool;
    // Start payload building
    let new_epoch = attrs.epoch.unwrap();
    let (blocktime, id) = {
        let engine_driver = engine_driver.read().await;
        // Validate chain head consistency, if concrete target provided.
        // This ensures that the attributes build on the head expected by the sequencer.
        validate_head_consistency(&engine_driver, target, "start_payload".to_string())?;
        (
            engine_driver.blocktime,
            engine_driver.start_payload_building(attrs.clone()).await?,
        )
    };
    // If we're including transaction from txpool, wait for the blocktime to pass.
    if !no_tx_pool {
        sleep(Duration::from_secs(blocktime)).await;
    }
    let engine_driver = engine_driver.read().await;
    // Validate chain head consistency again (probably non-essential, but avoids unnecessary work).
    validate_head_consistency(&engine_driver, target, "finalize_payload".to_string())?;
    // Finalize payload building.
    let new_head = engine_driver.finalize_payload_building(id).await?;
    Ok((new_head, new_epoch))
}

// Validates that the head matches the expected target head (if provided).
fn validate_head_consistency<E: Engine>(
    engine_driver: &EngineDriver<E>,
    target: &ChainHeadType,
    context: String,
) -> Result<(), EngineDriverError> {
    match target {
        ChainHeadType::Unsafe(info) if &engine_driver.unsafe_head != info => {
            Err(EngineDriverError::UnsafeHeadMismatch(
                context,
                info.hash,
                engine_driver.unsafe_head.hash,
            ))
        }
        _ => Ok(()),
    }
}

impl<E: Engine> EngineDriver<E> {
    /// Only use this if you are the only owner.
    /// Use [mod::handle_attributes] for multi-owner concurrent cases.
    pub async fn handle_attributes(
        &mut self,
        attributes: PayloadAttributes,
        update_safe: bool,
    ) -> Result<()> {
        match self.determine_action(&attributes).await? {
            Action::Skip(info) => self.skip_attributes(attributes, info).await,
            Action::Process(reorg) if reorg => {
                self.update_unsafe_head(self.safe_head, self.safe_epoch);
                self.process_attributes(attributes.clone(), reorg).await
            }
            Action::Process(_) => self.process_attributes(attributes, update_safe).await,
        }
    }

    pub async fn handle_unsafe_payload(&mut self, payload: &ExecutionPayload) -> Result<()> {
        self.push_payload(payload.clone()).await?;
        self.unsafe_head = payload.into();
        // TODO: inspect payload so we can set unsafe_epoch.
        self.update_forkchoice().await?;

        tracing::info!(
            "head updated: {} {:?}",
            self.unsafe_head.number,
            self.unsafe_head.hash,
        );

        Ok(())
    }

    /// Starts the payload building process via an Engine API call to `forkchoice_updated`.
    async fn start_payload_building(&self, attributes: PayloadAttributes) -> Result<U64> {
        let forkchoice = self.create_forkchoice_state();

        let update = self
            .engine
            .forkchoice_updated(forkchoice, Some(attributes))
            .await?;

        if update.payload_status.status != Status::Valid {
            let err = update.payload_status.validation_error.unwrap_or_default();
            eyre::bail!(format!("invalid payload attributes: {}", err));
        }

        update
            .payload_id
            .ok_or(eyre::eyre!("engine did not return payload id"))
    }

    /// Finalizes the payload building process via Engine API calls to
    /// `get_payload` and `new_payload`. Returns the resulting new head.
    pub async fn finalize_payload_building(&self, id: U64) -> Result<BlockInfo> {
        let payload = self.engine.get_payload(id).await?;
        tracing::info!(
            "built payload: block#={} hash={} ts={} #txs={}",
            payload.block_number,
            payload.block_hash,
            payload.timestamp,
            payload.transactions.len(),
        );
        let new_head = BlockInfo {
            number: payload.block_number.as_u64(),
            hash: payload.block_hash,
            parent_hash: payload.parent_hash,
            timestamp: payload.timestamp.as_u64(),
        };
        self.push_payload(payload).await?;
        Ok(new_head)
    }

    pub fn update_unsafe_head(&mut self, head: BlockInfo, epoch: Epoch) {
        tracing::trace!("updated unsafe: {}", self.unsafe_head.number);
        self.unsafe_head = head;
        self.unsafe_epoch = epoch;
    }

    pub fn update_safe_head(&mut self, head: BlockInfo, epoch: Epoch, reorg_unsafe: bool) {
        if self.safe_head != head {
            self.safe_head = head;
            self.safe_epoch = epoch;
        }
        if reorg_unsafe || self.safe_head.number > self.unsafe_head.number {
            tracing::trace!("updating unsafe to safe");
            self.update_unsafe_head(self.safe_head, self.safe_epoch);
        }
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
            .map_err(|e| tracing::error!("engine not ready yet: {:?}", e))
            .is_ok()
    }

    /// Determines the action to take on the given attributes.
    pub async fn determine_action(&self, attributes: &PayloadAttributes) -> Result<Action> {
        match self.block_at(attributes.timestamp.as_u64()).await {
            Some(block) if should_skip(&block, attributes)? => {
                Ok(Action::Skip(BlockInfo::try_from(block)?))
            }
            Some(_) => Ok(Action::Process(true)),
            _ => Ok(Action::Process(false)),
        }
    }

    async fn process_attributes(
        &mut self,
        attributes: PayloadAttributes,
        update_safe: bool,
    ) -> Result<()> {
        let new_epoch = *attributes.epoch.as_ref().unwrap();

        let payload = self.build_payload(attributes).await?;
        tracing::info!(
            "built payload: ts={} block#={} hash={}",
            payload.timestamp,
            payload.block_number,
            payload.block_hash
        );
        let new_head = BlockInfo {
            number: payload.block_number.as_u64(),
            hash: payload.block_hash,
            parent_hash: payload.parent_hash,
            timestamp: payload.timestamp.as_u64(),
        };

        self.push_payload(payload).await?;
        if update_safe {
            self.update_safe_head(new_head, new_epoch, true);
        } else {
            self.update_unsafe_head(new_head, new_epoch);
        }
        self.update_forkchoice().await?;

        Ok(())
    }

    async fn skip_attributes(
        &mut self,
        attributes: PayloadAttributes,
        new_head: BlockInfo,
    ) -> Result<()> {
        let new_epoch = *attributes.epoch.as_ref().unwrap();
        self.update_safe_head(new_head, new_epoch, false);
        self.update_forkchoice().await?;

        Ok(())
    }

    async fn build_payload(&self, attributes: PayloadAttributes) -> Result<ExecutionPayload> {
        let id = self.start_payload_building(attributes).await?;
        self.engine.get_payload(id).await
    }

    pub async fn push_payload(&self, payload: ExecutionPayload) -> Result<()> {
        let status = self.engine.new_payload(payload).await?;
        if status.status != Status::Valid && status.status != Status::Accepted {
            eyre::bail!("invalid execution payload");
        }

        Ok(())
    }

    pub async fn update_forkchoice(&self) -> Result<()> {
        let forkchoice = self.create_forkchoice_state();
        tracing::info!(
            "updating fc: head={} safe={} finalized={}",
            self.unsafe_head.number,
            self.safe_head.number,
            self.finalized_head.number,
        );

        let update = self.engine.forkchoice_updated(forkchoice, None).await?;
        if update.payload_status.status != Status::Valid {
            eyre::bail!(
                "could not accept new forkchoice: {:?}",
                update.payload_status.validation_error
            );
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
        finalized_head: HeadInfo,
        safe_head: HeadInfo,
        unsafe_head: HeadInfo,
        provider: Provider<Http>,
        config: &Arc<Config>,
    ) -> Result<Self> {
        let engine = Arc::new(EngineApi::new(&config.l2_engine_url, &config.jwt_secret));

        Ok(Self {
            engine,
            provider,
            blocktime: config.chain.blocktime,
            unsafe_head: unsafe_head.l2_block_info,
            unsafe_epoch: unsafe_head.l1_epoch,
            safe_head: safe_head.l2_block_info,
            safe_epoch: safe_head.l1_epoch,
            finalized_head: finalized_head.l2_block_info,
            finalized_epoch: finalized_head.l1_epoch,
        })
    }
}
