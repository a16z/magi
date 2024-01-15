use std::{
    process,
    sync::{mpsc::Receiver, Arc, RwLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use arc_swap::ArcSwap;

use ethers::{
    providers::{Http, Provider},
    types::Address,
};
use eyre::Result;

use libp2p::gossipsub::IdentTopic;
use reqwest::Url;
use thiserror::Error;
use tokio::{
    sync::watch::{self, Sender},
    time::sleep,
};

use crate::{
    config::{Config, SequencerConfig},
    derive::{state::State, Pipeline},
    driver::info::HeadInfoQuery,
    engine::{Engine, EngineApi, ExecutionPayload},
    l1::{BlockUpdate, ChainWatcher, L1Info},
    network::{
        handlers::{block_handler::BlockHandler, Handler},
        service::Service,
    },
    rpc,
    telemetry::metrics,
    types::common::{BlockInfo, Epoch},
    types::rpc::SyncStatus,
};

use self::engine_driver::EngineDriver;

mod engine_driver;
mod info;
mod types;
pub use types::*;

/// Driver is responsible for advancing the execution node by feeding
/// the derived chain into the engine API
pub struct Driver<E: Engine> {
    /// The derivation pipeline
    pipeline: Pipeline,
    /// The engine driver
    engine_driver: EngineDriver<E>,
    /// List of unfinalized L2 blocks with their epochs, L1 inclusions, and sequence numbers
    unfinalized_blocks: Vec<(BlockInfo, Epoch, u64, u64)>,
    /// Current finalized L1 block
    finalized_l1_block: BlockInfo,
    /// Current head L1 block
    head_l1_block: BlockInfo,
    /// Current safe L1 block
    safe_l1_block: BlockInfo,
    /// List of unsafe blocks that have not been applied yet
    future_unsafe_blocks: Vec<ExecutionPayload>,
    /// State struct to keep track of global state
    state: Arc<RwLock<State>>,
    /// L1 chain watcher
    chain_watcher: ChainWatcher,
    /// Channel to receive the shutdown signal from
    shutdown_recv: watch::Receiver<bool>,
    /// Channel to receive unsafe block from
    unsafe_block_recv: Receiver<ExecutionPayload>,
    /// Channel to send unsafe signer updated to block handler
    unsafe_block_signer_sender: Sender<Address>,
    /// Networking service
    network_service: Option<Service>,
    /// Channel timeout length.
    channel_timeout: u64,
    // Sender of the P2P broadcast channel.
    p2p_sender: tokio::sync::mpsc::Sender<ExecutionPayload>,
    // Receiver of the P2P broadcast channel.
    p2p_receiver: Option<tokio::sync::mpsc::Receiver<ExecutionPayload>>,
    /// L2 Block time.
    block_time: u64,
    /// Max sequencer drift.
    max_seq_drift: u64,
    /// Sequener config.
    sequencer_config: Option<SequencerConfig>,
    /// The Magi sync status.
    sync_status: Arc<ArcSwap<SyncStatus>>,
}

impl Driver<EngineApi> {
    pub async fn from_config(config: Config, shutdown_recv: watch::Receiver<bool>) -> Result<Self> {
        let client = reqwest::ClientBuilder::new()
            .timeout(Duration::from_secs(5))
            .build()?;

        let http = Http::new_with_client(Url::parse(&config.l2_rpc_url)?, client);
        let provider = Provider::new(http);
        let fetcher = info::HeadInfoFetcher::from(&provider);

        let heads = HeadInfoQuery::get_heads(&fetcher, &config.chain).await?;

        tracing::info!("starting finalized from head: {:?}", heads.finalized.head);

        let l1_start_block =
            get_l1_start_block(heads.finalized.epoch.number, config.chain.channel_timeout);

        let config = Arc::new(config);
        let chain_watcher: ChainWatcher =
            ChainWatcher::new(l1_start_block, heads.finalized.head.number, config.clone())?;

        let state = State::new(
            heads.finalized.head,
            heads.finalized.epoch,
            heads.latest.head,
            heads.latest.epoch,
            &provider,
            Arc::clone(&config.chain),
        )
        .await;
        let state = Arc::new(RwLock::new(state));

        let sync_status = Arc::new(ArcSwap::from_pointee(Default::default()));

        let engine_driver =
            EngineDriver::new(heads.finalized, heads.safe, heads.latest, provider, &config)?;
        let pipeline = Pipeline::new(
            state.clone(),
            Arc::clone(&config.chain),
            heads.finalized.seq_number,
            heads.latest.seq_number,
        )?;

        let _addr = rpc::run_server(config.clone(), sync_status.clone()).await?;

        let (unsafe_block_signer_sender, unsafe_block_signer_recv) =
            watch::channel(config.chain.genesis.system_config.unsafe_block_signer);

        let (block_handler, unsafe_block_recv) =
            BlockHandler::new(config.chain.l2_chain_id, unsafe_block_signer_recv);

        let service = Service::new(
            config.p2p_listen,
            config.chain.l2_chain_id,
            config.p2p_bootnodes.clone(),
            config.p2p_secret_key.clone(),
            config.p2p_sequencer_secret_key.clone(),
            IdentTopic::new(block_handler.topics()[0].to_string()),
        )
        .add_handler(Box::new(block_handler));

        // channel for sending new blocks to peers
        let (p2p_sender, p2p_receiver) = tokio::sync::mpsc::channel(1_000);

        Ok(Self {
            engine_driver,
            pipeline,
            unfinalized_blocks: Vec::new(),
            finalized_l1_block: Default::default(),
            head_l1_block: Default::default(),
            safe_l1_block: Default::default(),
            future_unsafe_blocks: Vec::new(),
            state,
            chain_watcher,
            shutdown_recv,
            unsafe_block_recv,
            unsafe_block_signer_sender,
            network_service: Some(service),
            channel_timeout: config.chain.channel_timeout,
            p2p_receiver: Some(p2p_receiver),
            p2p_sender,
            block_time: config.chain.block_time,
            max_seq_drift: config.chain.max_sequencer_drift,
            sequencer_config: config.sequencer.clone(),
            sync_status,
        })
    }
}

/// Custom error for sequencer.
#[derive(Debug, Error)]
enum SequencerErr {
    #[error("out of sync with L1")]
    OutOfSyncL1,

    #[error("past sequencer drift")]
    PastSeqDrift,

    #[error("sequencer critical error: {0}")]
    Critical(String),
}

impl<E: Engine> Driver<E> {
    /// Runs the Driver
    pub async fn start(&mut self) -> Result<()> {
        self.await_engine_ready().await;
        self.chain_watcher.start()?;

        loop {
            self.check_shutdown().await;

            if let Err(err) = self.advance().await {
                tracing::error!("fatal error: {:?}", err);
                self.shutdown().await;
            }
        }
    }

    /// Shuts down the driver
    pub async fn shutdown(&self) {
        process::exit(0);
    }

    /// Checks for shutdown signal and shuts down if received
    async fn check_shutdown(&self) {
        if *self.shutdown_recv.borrow() {
            self.shutdown().await;
        }
    }

    async fn await_engine_ready(&self) {
        while !self.engine_driver.engine_ready().await {
            self.check_shutdown().await;
            sleep(Duration::from_secs(1)).await;
        }
    }

    /// Attempts to advance the execution node forward using either L1 info our
    /// blocks received on the p2p network.
    async fn advance(&mut self) -> Result<()> {
        self.advance_safe_head().await?;
        self.advance_unsafe_head().await?;
        self.run_sequencer_step().await?;

        self.update_finalized();
        self.update_sync_status().await?;
        self.update_metrics();

        self.try_start_networking()?;

        Ok(())
    }

    /// Prepare data for generating next payload.
    fn prepare_block_data(
        &self,
        unsafe_epoch: Epoch,
        new_blocktime: u64,
    ) -> Result<(Epoch, L1Info)> {
        let state = self.state.read().expect("lock poisoned");

        // Check we are in sync with L1.
        let current_epoch = state
            .epoch_by_number(unsafe_epoch.number)
            .ok_or(SequencerErr::OutOfSyncL1)?;

        // Check past sequencer drift.
        let is_seq_drift = new_blocktime > current_epoch.timestamp + self.max_seq_drift;
        let next_epoch = state.epoch_by_number(current_epoch.number + 1);

        let origin_epoch = if let Some(next_epoch) = next_epoch {
            if new_blocktime >= next_epoch.timestamp {
                next_epoch
            } else {
                current_epoch
            }
        } else {
            if is_seq_drift {
                return Err(SequencerErr::PastSeqDrift.into());
            }

            // TODO: retrieve the next L1 block directly from L1 node?
            tracing::warn!("no next epoch found, current epoch used");

            current_epoch
        };

        let l1_info = state
            .l1_info_by_hash(origin_epoch.hash)
            .ok_or(SequencerErr::Critical(
                "can't find l1 info for origin epoch during block building".to_string(),
            ))?;

        Ok((origin_epoch, l1_info.clone()))
    }

    /// Runs the sequencer step.
    /// Produces a block if the conditions are met.
    /// If successful the block would be signed by sequencer and shared by P2P.
    async fn run_sequencer_step(&mut self) -> Result<()> {
        if let Some(seq_config) = self.sequencer_config.as_ref() {
            // Get unsafe head to build a new block on top of it.
            let unsafe_head = self.engine_driver.unsafe_info.head;
            let unsafe_epoch = self.engine_driver.unsafe_info.epoch;

            if seq_config.max_safe_lag() > 0 {
                // Check max safe lag, and in case delay produce blocks.
                if self.engine_driver.safe_info.head.number + seq_config.max_safe_lag()
                    <= unsafe_head.number
                {
                    tracing::debug!("max safe lag reached, waiting for safe block...");
                    return Ok(());
                }
            }

            // Next block timestamp.
            let new_blocktime = unsafe_head.timestamp + self.block_time;

            // Check if we can generate block and time passed.
            let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
            if new_blocktime > now {
                return Ok(());
            }

            // Prepare data (origin epoch and l1 information) for next block.
            let (epoch, l1_info) = match self.prepare_block_data(unsafe_epoch, new_blocktime) {
                Ok((epoch, l1_info)) => (epoch, l1_info),
                Err(err) => match err.downcast()? {
                    SequencerErr::OutOfSyncL1 => {
                        tracing::debug!("out of sync L1 {:?}", unsafe_epoch);
                        return Ok(());
                    }
                    SequencerErr::Critical(msg) => eyre::bail!(msg),
                    SequencerErr::PastSeqDrift => eyre::bail!(
                        "failed to find next L1 origin for new block under past sequencer drifted"
                    ),
                },
            };

            let block_num = unsafe_head.number + 1;
            tracing::info!(
                "attempt to build a payload {} {} {:?}",
                block_num,
                new_blocktime,
                epoch,
            );

            let mut attributes =
                self.pipeline
                    .derive_attributes_for_next_block(epoch, &l1_info, new_blocktime);

            tracing::trace!("produced payload attributes {} {:?}", block_num, attributes);

            attributes.no_tx_pool = new_blocktime > epoch.timestamp + self.max_seq_drift;

            if attributes.no_tx_pool {
                tracing::warn!("tx pool disabled because of max sequencer drift");
            }

            let payload = self.engine_driver.build_payload(attributes).await?;

            tracing::trace!("produced payload {} {:?}", block_num, payload);

            self.engine_driver.handle_unsafe_payload(&payload).await?;
            self.p2p_sender.send(payload).await?;

            self.state
                .write()
                .expect("lock posioned")
                .update_unsafe_head(
                    self.engine_driver.unsafe_info.head,
                    self.engine_driver.unsafe_info.epoch,
                );
        }

        Ok(())
    }

    /// Attempts to advance the execution node forward one L1 block using derived
    /// L1 data. Errors if the most recent PayloadAttributes from the pipeline
    /// does not successfully advance the node
    async fn advance_safe_head(&mut self) -> Result<()> {
        self.handle_next_block_update().await?;
        self.update_state_head()?;

        for next_attributes in self.pipeline.by_ref() {
            let l1_inclusion_block = next_attributes
                .l1_inclusion_block
                .ok_or(eyre::eyre!("attributes without inclusion block"))?;

            let seq_number = next_attributes
                .seq_number
                .ok_or(eyre::eyre!("attributes without seq number"))?;

            self.engine_driver
                .handle_attributes(next_attributes)
                .await?;

            tracing::info!(
                "safe head updated: {} {:?}",
                self.engine_driver.safe_info.head.number,
                self.engine_driver.safe_info.head.hash,
            );

            let new_safe_head = self.engine_driver.safe_info.head;
            let new_safe_epoch = self.engine_driver.safe_info.epoch;

            self.state
                .write()
                .expect("lock poisoned")
                .update_safe_head(new_safe_head, new_safe_epoch);

            let unfinalized_entry = (
                new_safe_head,
                new_safe_epoch,
                l1_inclusion_block,
                seq_number,
            );

            self.unfinalized_blocks.push(unfinalized_entry);
        }

        Ok(())
    }

    async fn advance_unsafe_head(&mut self) -> Result<()> {
        while let Ok(payload) = self.unsafe_block_recv.try_recv() {
            self.future_unsafe_blocks.push(payload);
        }

        self.future_unsafe_blocks.retain(|payload| {
            let unsafe_block_num = payload.block_number.as_u64();
            let synced_block_num = self.engine_driver.unsafe_info.head.number;

            unsafe_block_num > synced_block_num && unsafe_block_num - synced_block_num < 1024
        });

        let next_unsafe_payload = self
            .future_unsafe_blocks
            .iter()
            .find(|p| p.parent_hash == self.engine_driver.unsafe_info.head.hash);

        if let Some(payload) = next_unsafe_payload {
            if let Err(err) = self.engine_driver.handle_unsafe_payload(payload).await {
                tracing::debug!("Error processing unsafe payload: {err}");
            } else {
                self.state
                    .write()
                    .expect("lock poisoned")
                    .update_unsafe_head(
                        self.engine_driver.unsafe_info.head,
                        self.engine_driver.unsafe_info.epoch,
                    );
            }
        }

        Ok(())
    }

    fn update_state_head(&self) -> Result<()> {
        let mut state = self.state.write().expect("lock poisoned");

        state.update_safe_head(
            self.engine_driver.safe_info.head,
            self.engine_driver.safe_info.epoch,
        );

        Ok(())
    }

    /// Ingests the next update from the block update channel
    async fn handle_next_block_update(&mut self) -> Result<()> {
        let next = self.chain_watcher.try_recv_from_channel();

        if let Ok(update) = next {
            match update {
                BlockUpdate::NewBlock(l1_info) => {
                    let num = l1_info.block_info.number;

                    self.unsafe_block_signer_sender
                        .send(l1_info.system_config.unsafe_block_signer)?;

                    self.pipeline
                        .push_batcher_transactions(l1_info.batcher_transactions.clone(), num)?;

                    self.state
                        .write()
                        .expect("lock poisoned")
                        .update_l1_info(*l1_info);
                }
                BlockUpdate::Reorg => {
                    tracing::warn!("reorg detected, purging pipeline");

                    let finalized_info = self.engine_driver.finalized_info;

                    let l1_start_block =
                        get_l1_start_block(finalized_info.epoch.number, self.channel_timeout);

                    self.unfinalized_blocks.clear();
                    self.chain_watcher
                        .restart(l1_start_block, finalized_info.head.number)?;

                    self.state
                        .write()
                        .expect("lock poisoned")
                        .purge(finalized_info.head, finalized_info.epoch);

                    self.pipeline.purge()?;
                    self.engine_driver.reorg();
                }
                BlockUpdate::FinalityUpdate(block) => self.finalized_l1_block = block,
                BlockUpdate::HeadUpdate(block) => self.head_l1_block = block,
                BlockUpdate::SafetyUpdate(block) => self.safe_l1_block = block,
            }
        }

        Ok(())
    }

    fn update_finalized(&mut self) {
        let new_finalized = self
            .unfinalized_blocks
            .iter()
            .filter(|(_, _, inclusion, seq)| {
                *inclusion <= self.finalized_l1_block.number && *seq == 0
            })
            .last();

        if let Some((head, epoch, _, seq)) = new_finalized {
            self.engine_driver.update_finalized(*head, *epoch, *seq);
        }

        self.unfinalized_blocks
            .retain(|(_, _, inclusion, _)| *inclusion > self.finalized_l1_block.number);
    }

    fn try_start_networking(&mut self) -> Result<()> {
        if let Some(service) = self.network_service.take() {
            let p2p_receiver = self
                .p2p_receiver
                .take()
                .expect("The channel is not initialized");
            service.start(p2p_receiver)?;
        }

        Ok(())
    }

    fn update_metrics(&self) {
        metrics::FINALIZED_HEAD.set(self.engine_driver.finalized_info.head.number as i64);
        metrics::SAFE_HEAD.set(self.engine_driver.safe_info.head.number as i64);
        metrics::SYNCED.set(self.synced() as i64);
    }

    fn synced(&self) -> bool {
        !self.unfinalized_blocks.is_empty()
    }

    async fn update_sync_status(&self) -> Result<()> {
        let state = self.state.read().expect("lock poisoned");

        let current_l1_info = state.l1_info_current();

        if let Some(current_l1_info) = current_l1_info {
            let finalized_l1 = self.finalized_l1_block;
            let head_l1 = self.head_l1_block;
            let safe_l1 = self.safe_l1_block;
            let queued_unsafe_block = self.get_queued_unsafe_block();

            let new_status = SyncStatus::new(
                current_l1_info.block_info.into(),
                finalized_l1,
                head_l1,
                safe_l1,
                self.engine_driver.unsafe_info,
                self.engine_driver.safe_info,
                self.engine_driver.finalized_info,
                queued_unsafe_block,
                self.engine_driver.sync_info,
            )?;

            self.sync_status.store(Arc::new(new_status));
        }

        Ok(())
    }

    fn get_queued_unsafe_block(&self) -> Option<&ExecutionPayload> {
        self.future_unsafe_blocks
            .iter()
            .min_by_key(|payload| payload.block_number.as_u64())
    }
}

/// Retrieves the L1 start block number.
/// If an overflow occurs during subtraction, the function returns the genesis block #0.
fn get_l1_start_block(epoch_number: u64, channel_timeout: u64) -> u64 {
    epoch_number.saturating_sub(channel_timeout)
}

#[cfg(test)]
mod tests {
    use ethers::{
        middleware::Middleware,
        providers::Http,
        types::{BlockId, BlockNumber},
    };
    use eyre::Result;
    use tokio::sync::watch::channel;

    use crate::config::ChainConfig;

    use super::*;

    #[tokio::test]
    async fn test_new_driver_from_finalized_head() -> Result<()> {
        let rpc_env = std::env::var("L1_TEST_RPC_URL");
        let l2_rpc_env = std::env::var("L2_TEST_RPC_URL");
        let (rpc, l2_rpc) = match (rpc_env, l2_rpc_env) {
            (Ok(rpc), Ok(l2_rpc)) => (rpc, l2_rpc),
            (rpc_env, l2_rpc_env) => {
                eprintln!("Test ignored: `test_new_driver_from_finalized_head`, rpc: {rpc_env:?}, l2_rpc: {l2_rpc_env:?}");
                return Ok(());
            }
        };

        // threshold for cases, when new blocks generated
        let max_difference = 500;

        let config = Config {
            chain: ChainConfig::optimism_goerli(),
            l1_rpc_url: rpc,
            l2_rpc_url: l2_rpc.clone(),
            jwt_secret: "d195a64e08587a3f1560686448867220c2727550ce3e0c95c7200d0ade0f9167"
                .to_owned(),
            checkpoint_sync_url: Some(l2_rpc),
            ..Config::default()
        };

        let (_shutdown_sender, shutdown_recv) = channel(false);

        let block_id = BlockId::Number(BlockNumber::Finalized);
        let provider = Provider::<Http>::try_from(config.l2_rpc_url.clone())?;
        let finalized_block = provider.get_block(block_id).await?.unwrap();

        let driver = Driver::from_config(config, shutdown_recv).await?;

        let finalized_head_num = driver.engine_driver.finalized_info.head.number;
        let block_num = finalized_block.number.unwrap().as_u64();

        let difference = if finalized_head_num > block_num {
            finalized_head_num - block_num
        } else {
            block_num - finalized_head_num
        };

        assert!(
            difference <= max_difference,
            "Difference between finalized_head_num ({finalized_head_num}) and block_num ({block_num}) \
            exceeds the threshold of {max_difference}",
        );

        Ok(())
    }
}
