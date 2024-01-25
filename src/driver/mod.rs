use std::{
    process,
    sync::{mpsc::Receiver, Arc, RwLock},
    time::Duration,
};

use ethers::{
    providers::{Http, Provider},
    types::{Address, BlockNumber},
};
use eyre::Result;
use reqwest::Url;
use tokio::{
    sync::{
        watch::{self, Sender},
        RwLock as TokioRwLock,
    },
    time::sleep,
};

use crate::{
    common::{BlockInfo, Epoch},
    config::Config,
    derive::{state::State, Pipeline},
    engine::{Engine, EngineApi, ExecutionPayload},
    l1::{BlockUpdate, ChainWatcher},
    network::{handlers::block_handler::BlockHandler, service::Service},
    rpc, specular,
    telemetry::metrics,
};

use self::engine_driver::{handle_attributes, ChainHeadType, EngineDriver};

pub mod engine_driver;
mod info;
pub mod sequencing;
mod types;
pub use types::*;

/// Driver is responsible for advancing the execution node by feeding
/// the derived chain into the engine API
pub struct Driver<E: Engine> {
    /// The derivation pipeline
    pipeline: Pipeline,
    /// The engine driver
    pub engine_driver: Arc<TokioRwLock<EngineDriver<E>>>,
    /// List of unfinalized L2 blocks with their epochs, L1 inclusions, and sequence numbers
    unfinalized_blocks: Vec<(BlockInfo, Epoch, u64, u64)>,
    /// Current finalized L1 block number
    finalized_l1_block_number: u64,
    /// List of unsafe blocks that have not been applied yet
    future_unsafe_blocks: Vec<ExecutionPayload>,
    /// State struct to keep track of global state
    pub state: Arc<RwLock<State>>,
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
    /// Channel timeout length
    channel_timeout: u64,
}

impl Driver<EngineApi> {
    pub async fn from_config(config: Config, shutdown_recv: watch::Receiver<bool>) -> Result<Self> {
        let client = reqwest::ClientBuilder::new()
            .timeout(Duration::from_secs(5))
            .build()?;

        let http = Http::new_with_client(Url::parse(&config.l2_rpc_url)?, client);
        let provider = Provider::new(http);

        // TODO: cleanup
        macro_rules! get_head_info {
            ($bn:expr, $fb:expr) => {
                if config.chain.meta.enable_deposited_txs {
                    info::HeadInfoQuery::get_head_info(
                        &info::HeadInfoFetcher::from(&provider),
                        &config,
                        $bn,
                    )
                    .await
                } else {
                    specular::info::HeadInfoQuery::get_head_info(
                        &specular::info::HeadInfoFetcher::from(&provider),
                        &config,
                        $bn,
                        $fb,
                    )
                    .await
                }
            };
        }

        let finalized_head = get_head_info!(BlockNumber::Finalized, None);
        let safe_head = get_head_info!(BlockNumber::Safe, Some(finalized_head.clone()));
        let latest_head = get_head_info!(BlockNumber::Latest, Some(safe_head.clone()));

        tracing::info!(
            "starting from fc: finalized {:?}, safe {:?}, latest {:?}",
            finalized_head.l2_block_info.hash,
            safe_head.l2_block_info.hash,
            latest_head.l2_block_info.hash
        );

        let finalized_l2_block = finalized_head.l2_block_info;
        let finalized_epoch = finalized_head.l1_epoch;
        let finalized_seq = finalized_head.sequence_number;

        let l1_start_block =
            get_l1_start_block(finalized_epoch.number, config.chain.channel_timeout);

        let config = Arc::new(config);
        let chain_watcher =
            ChainWatcher::new(l1_start_block, finalized_l2_block.number, config.clone())?;

        let state = Arc::new(RwLock::new(State::new(
            safe_head.l2_block_info,
            safe_head.l1_epoch,
            config.clone(),
        )));

        let engine_driver =
            EngineDriver::new(finalized_head, safe_head, latest_head, provider, &config)?;
        let pipeline = Pipeline::new(state.clone(), config.clone(), finalized_seq)?;

        let _addr = rpc::run_server(config.clone()).await?;

        let (unsafe_block_signer_sender, unsafe_block_signer_recv) =
            watch::channel(config.chain.system_config.unsafe_block_signer);

        let (block_handler, unsafe_block_recv) =
            BlockHandler::new(config.chain.l2_chain_id, unsafe_block_signer_recv);

        let service = Service::new("0.0.0.0:9876".parse()?, config.chain.l2_chain_id)
            .add_handler(Box::new(block_handler));

        let engine_driver = Arc::new(TokioRwLock::new(engine_driver));

        Ok(Self {
            engine_driver,
            pipeline,
            unfinalized_blocks: Vec::new(),
            finalized_l1_block_number: 0,
            future_unsafe_blocks: Vec::new(),
            state,
            chain_watcher,
            shutdown_recv,
            unsafe_block_recv,
            unsafe_block_signer_sender,
            network_service: Some(service),
            channel_timeout: config.chain.channel_timeout,
        })
    }
}

impl<E: Engine> Driver<E> {
    /// Runs the Driver
    pub async fn start(&mut self) -> Result<()> {
        tracing::trace!("starting chain watcher...");
        self.chain_watcher.start()?;
        tracing::trace!("chain watcher started; advancing driver...");
        self.await_engine_ready().await;
        self.engine_driver.read().await.update_forkchoice().await?;
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
        while !self.engine_driver.read().await.engine_ready().await {
            self.check_shutdown().await;
            sleep(Duration::from_secs(1)).await;
        }
    }

    /// Attempts to advance the execution node forward using either L1 info our
    /// blocks received on the p2p network.
    async fn advance(&mut self) -> Result<()> {
        // TODO: `await_engine_ready` was moved from `start` to here.
        // This is a hack, due to possible lock contention bug (to be reverted).
        self.await_engine_ready().await;
        self.advance_safe_head().await?;
        self.advance_unsafe_head().await?;
        self.update_finalized().await;
        self.update_metrics().await;
        self.try_start_networking()?;

        Ok(())
    }

    /// Attempts to advance the execution node forward one L1 block using derived
    /// L1 data. Errors if the most recent PayloadAttributes from the pipeline
    /// does not successfully advance the node
    async fn advance_safe_head(&mut self) -> Result<()> {
        self.handle_next_block_update().await?;
        self.update_state_head().await?;

        for next_attributes in self.pipeline.by_ref() {
            let l1_inclusion_block = next_attributes
                .l1_inclusion_block
                .ok_or(eyre::eyre!("attributes without inclusion block"))?;

            let seq_number = next_attributes
                .seq_number
                .ok_or(eyre::eyre!("attributes without seq number"))?;

            handle_attributes(
                next_attributes,
                &ChainHeadType::Safe,
                self.engine_driver.clone(),
            )
            .await?;

            let engine_driver = self.engine_driver.read().await;
            tracing::trace!(
                "safe head updated: {} {}",
                engine_driver.safe_head.number,
                engine_driver.safe_head.hash,
            );

            let new_safe_head = engine_driver.safe_head;
            let new_safe_epoch = engine_driver.safe_epoch;

            self.state
                .write()
                .map_err(|_| eyre::eyre!("lock poisoned"))?
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

        let engine_driver = self.engine_driver.read().await;
        self.future_unsafe_blocks.retain(|payload| {
            let unsafe_block_num = payload.block_number.as_u64();
            let synced_block_num = engine_driver.unsafe_head.number;

            unsafe_block_num > synced_block_num && unsafe_block_num - synced_block_num < 1024
        });

        let next_unsafe_payload = self
            .future_unsafe_blocks
            .iter()
            .find(|p| p.parent_hash == engine_driver.unsafe_head.hash);

        if let Some(payload) = next_unsafe_payload {
            engine_driver.push_payload(payload.clone()).await?;
            drop(engine_driver);
            // TODO: update epoch of unsafe head.
            self.engine_driver.write().await.unsafe_head = payload.into();
            self.engine_driver.read().await.update_forkchoice().await?;
        }

        Ok(())
    }

    async fn update_state_head(&self) -> Result<()> {
        let engine_driver = self.engine_driver.read().await;
        let mut state = self
            .state
            .write()
            .map_err(|_| eyre::eyre!("lock poisoned"))?;
        state.update_safe_head(engine_driver.safe_head, engine_driver.safe_epoch);

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

                    tracing::info!(
                        "pushing into pipeline: l1_block#={} #batcher_txs={}",
                        num,
                        l1_info.batcher_transactions.len()
                    );
                    self.pipeline
                        .push_batcher_transactions(l1_info.batcher_transactions.clone(), num)?;

                    self.state
                        .write()
                        .map_err(|_| eyre::eyre!("lock poisoned"))?
                        .update_l1_info(*l1_info);
                }
                BlockUpdate::Reorg => {
                    tracing::warn!("reorg detected, purging pipeline");

                    self.unfinalized_blocks.clear();

                    let mut engine_driver = self.engine_driver.write().await;
                    let l1_start_block = get_l1_start_block(
                        engine_driver.finalized_epoch.number,
                        self.channel_timeout,
                    );

                    self.chain_watcher
                        .restart(l1_start_block, engine_driver.finalized_head.number)?;

                    self.state
                        .write()
                        .map_err(|_| eyre::eyre!("lock poisoned"))?
                        .purge(engine_driver.finalized_head, engine_driver.finalized_epoch);

                    self.pipeline.purge()?;
                    engine_driver.reorg();
                }
                BlockUpdate::FinalityUpdate(num) => {
                    self.finalized_l1_block_number = num;
                }
            }
        }

        Ok(())
    }

    async fn update_finalized(&mut self) {
        let new_finalized = self
            .unfinalized_blocks
            .iter()
            .filter(|(_, _, inclusion, seq)| {
                *inclusion <= self.finalized_l1_block_number && *seq == 0
            })
            .last();

        if let Some((head, epoch, _, _)) = new_finalized {
            tracing::info!("updating finalized head: {:?}", head.number);
            self.engine_driver
                .write()
                .await
                .update_finalized(*head, *epoch);
        }

        self.unfinalized_blocks
            .retain(|(_, _, inclusion, _)| *inclusion > self.finalized_l1_block_number);
    }

    fn try_start_networking(&mut self) -> Result<()> {
        if self.synced() {
            if let Some(service) = self.network_service.take() {
                service.start()?;
            }
        }

        Ok(())
    }

    async fn update_metrics(&self) {
        let engine_driver = self.engine_driver.read().await;
        metrics::FINALIZED_HEAD.set(engine_driver.finalized_head.number as i64);
        metrics::SAFE_HEAD.set(engine_driver.safe_head.number as i64);
        metrics::SYNCED.set(self.synced() as i64);
    }

    fn synced(&self) -> bool {
        !self.unfinalized_blocks.is_empty()
    }
}

/// Retrieves the L1 start block number.
/// If an overflow occurs during subtraction, the function returns the genesis block #0.
fn get_l1_start_block(epoch_number: u64, channel_timeout: u64) -> u64 {
    epoch_number.saturating_sub(channel_timeout)
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, str::FromStr};

    use ethers::{
        providers::{Http, Middleware},
        types::{BlockId, BlockNumber},
    };
    use eyre::Result;
    use tokio::sync::watch::channel;

    use crate::config::{ChainConfig, CliConfig};

    use super::*;

    #[tokio::test]
    async fn test_new_driver_from_finalized_head() -> Result<()> {
        if std::env::var("L1_TEST_RPC_URL").is_ok() && std::env::var("L2_TEST_RPC_URL").is_ok() {
            let config_path = PathBuf::from_str("config.toml")?;
            let rpc = std::env::var("L1_TEST_RPC_URL")?;
            let l2_rpc = std::env::var("L2_TEST_RPC_URL")?;
            let cli_config = CliConfig {
                l1_rpc_url: Some(rpc.to_owned()),
                l2_rpc_url: Some(l2_rpc.to_owned()),
                l2_engine_url: None,
                jwt_secret: Some(
                    "d195a64e08587a3f1560686448867220c2727550ce3e0c95c7200d0ade0f9167".to_owned(),
                ),
                checkpoint_sync_url: Some(l2_rpc.to_owned()),
                rpc_port: None,
                devnet: false,
                local_sequencer: None,
            };
            let config = Config::new(&config_path, cli_config, ChainConfig::optimism_goerli());
            let (_shutdown_sender, shutdown_recv) = channel(false);

            let block_id = BlockId::Number(BlockNumber::Finalized);
            let provider = Provider::<Http>::try_from(config.l2_rpc_url.clone())?;
            let finalized_block = provider.get_block(block_id).await?.unwrap();

            let driver = Driver::from_config(config, shutdown_recv).await?;

            assert_eq!(
                driver.engine_driver.read().await.finalized_head.number,
                finalized_block.number.unwrap().as_u64()
            );
        }
        Ok(())
    }
}
