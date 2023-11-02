use std::{
    process,
    sync::{mpsc::Receiver, Arc, RwLock},
    time::Duration,
};

use ethers::{
    providers::{Http, Provider},
    types::Address,
};
use eyre::Result;
use reqwest::Url;
use tokio::{
    sync::watch::{self, Sender},
    time::sleep,
};

use crate::{
    common::{BlockInfo, Epoch},
    config::Config,
    derive::{state::State, Pipeline},
    engine::{Engine, EngineApi, ExecutionPayload},
    l1::{BlockUpdate, ChainWatcher},
    network::{handlers::block_handler::BlockHandler, service::Service},
    rpc,
    telemetry::metrics,
};

use self::engine_driver::EngineDriver;

mod engine_driver;
mod info;
pub mod sequencing;
mod types;
pub use types::*;

/// Driver is responsible for advancing the execution node by feeding
/// the derived chain into the engine API
pub struct Driver<E: Engine, S: sequencing::SequencingSource<E>> {
    /// The derivation pipeline
    pipeline: Pipeline,
    /// The engine driver
    engine_driver: EngineDriver<E>,
    /// List of unfinalized L2 blocks with their epochs, L1 inclusions, and sequence numbers
    unfinalized_blocks: Vec<(BlockInfo, Epoch, u64, u64)>,
    /// Current finalized L1 block number
    finalized_l1_block_number: u64,
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
    /// Channel timeout length
    channel_timeout: u64,
    /// Local sequencing source
    sequencing_src: Option<S>,
}

impl<S: sequencing::SequencingSource<EngineApi>> Driver<EngineApi, S> {
    pub async fn from_config(
        config: Config,
        shutdown_recv: watch::Receiver<bool>,
        sequencing_src: Option<S>,
    ) -> Result<Self> {
        let client = reqwest::ClientBuilder::new()
            .timeout(Duration::from_secs(5))
            .build()?;

        let http = Http::new_with_client(Url::parse(&config.l2_rpc_url)?, client);
        let provider = Provider::new(http);

        let head =
            info::HeadInfoQuery::get_head_info(&info::HeadInfoFetcher::from(&provider), &config)
                .await;

        let finalized_head = head.l2_block_info;
        let finalized_epoch = head.l1_epoch;
        let finalized_seq = head.sequence_number;

        tracing::info!("starting from head: {:?}", finalized_head.hash);

        let l1_start_block =
            get_l1_start_block(finalized_epoch.number, config.chain.channel_timeout);

        let config = Arc::new(config);
        let chain_watcher =
            ChainWatcher::new(l1_start_block, finalized_head.number, config.clone())?;

        let state = Arc::new(RwLock::new(State::new(
            finalized_head,
            finalized_epoch,
            config.clone(),
        )));

        let engine_driver = EngineDriver::new(finalized_head, finalized_epoch, provider, &config)?;
        let pipeline = Pipeline::new(state.clone(), config.clone(), finalized_seq)?;

        let _addr = rpc::run_server(config.clone()).await?;

        let (unsafe_block_signer_sender, unsafe_block_signer_recv) =
            watch::channel(config.chain.system_config.unsafe_block_signer);

        let (block_handler, unsafe_block_recv) =
            BlockHandler::new(config.chain.l2_chain_id, unsafe_block_signer_recv);

        let service = Service::new("0.0.0.0:9876".parse()?, config.chain.l2_chain_id)
            .add_handler(Box::new(block_handler));

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
            sequencing_src,
        })
    }
}

impl<E: Engine, S: sequencing::SequencingSource<E>> Driver<E, S> {
    /// Runs the Driver
    pub async fn start(&mut self) -> Result<()> {
        tracing::trace!("starting driver, waiting for engine...");
        self.await_engine_ready().await;
        tracing::trace!("engine ready; starting chain watcher...");
        self.chain_watcher.start()?;
        tracing::trace!("chain watcher started");

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
            tracing::trace!("waiting for engine ready...");
            self.check_shutdown().await;
            sleep(Duration::from_secs(1)).await;
        }
    }

    /// Attempts to advance the execution node forward using either L1 info our
    /// blocks received on the p2p network.
    async fn advance(&mut self) -> Result<()> {
        self.advance_safe_head().await?;
        self.advance_unsafe_head().await?;
        self.advance_unsafe_head_by_attributes().await?;

        self.update_finalized();
        self.update_metrics();
        self.try_start_networking()?;

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
                .handle_attributes(next_attributes, true)
                .await?;

            tracing::info!(
                "safe head updated: {} {:?}",
                self.engine_driver.safe_head.number,
                self.engine_driver.safe_head.hash,
            );

            let new_safe_head = self.engine_driver.safe_head;
            let new_safe_epoch = self.engine_driver.safe_epoch;

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

        self.future_unsafe_blocks.retain(|payload| {
            let unsafe_block_num = payload.block_number.as_u64();
            let synced_block_num = self.engine_driver.unsafe_head.number;

            unsafe_block_num > synced_block_num && unsafe_block_num - synced_block_num < 1024
        });

        let next_unsafe_payload = self
            .future_unsafe_blocks
            .iter()
            .find(|p| p.parent_hash == self.engine_driver.unsafe_head.hash);

        if let Some(payload) = next_unsafe_payload {
            _ = self.engine_driver.handle_unsafe_payload(payload).await;
        }

        Ok(())
    }

    /// Tries to process the next unbuilt payload attributes, building on the current forkchoice.
    async fn advance_unsafe_head_by_attributes(&mut self) -> Result<()> {
        let Some(sequencing_src) = &self.sequencing_src else {
            return Ok(());
        };
        let Some(attrs) = sequencing_src
            .get_next_attributes(&self.state, &self.engine_driver)
            .await?
        else {
            return Ok(());
        };
        self.engine_driver.handle_attributes(attrs, false).await?;
        Ok(())
    }

    fn update_state_head(&self) -> Result<()> {
        let mut state = self
            .state
            .write()
            .map_err(|_| eyre::eyre!("lock poisoned"))?;

        state.update_safe_head(self.engine_driver.safe_head, self.engine_driver.safe_epoch);

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
                        .map_err(|_| eyre::eyre!("lock poisoned"))?
                        .update_l1_info(*l1_info);
                }
                BlockUpdate::Reorg => {
                    tracing::warn!("reorg detected, purging pipeline");

                    self.unfinalized_blocks.clear();

                    let l1_start_block = get_l1_start_block(
                        self.engine_driver.finalized_epoch.number,
                        self.channel_timeout,
                    );

                    self.chain_watcher
                        .restart(l1_start_block, self.engine_driver.finalized_head.number)?;

                    self.state
                        .write()
                        .map_err(|_| eyre::eyre!("lock poisoned"))?
                        .purge(
                            self.engine_driver.finalized_head,
                            self.engine_driver.finalized_epoch,
                        );

                    self.pipeline.purge()?;
                    self.engine_driver.reorg();
                }
                BlockUpdate::FinalityUpdate(num) => {
                    self.finalized_l1_block_number = num;
                }
            }
        }

        Ok(())
    }

    fn update_finalized(&mut self) {
        let new_finalized = self
            .unfinalized_blocks
            .iter()
            .filter(|(_, _, inclusion, seq)| {
                *inclusion <= self.finalized_l1_block_number && *seq == 0
            })
            .last();

        if let Some((head, epoch, _, _)) = new_finalized {
            self.engine_driver.update_finalized(*head, *epoch);
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

    fn update_metrics(&self) {
        metrics::FINALIZED_HEAD.set(self.engine_driver.finalized_head.number as i64);
        metrics::SAFE_HEAD.set(self.engine_driver.safe_head.number as i64);
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

            let seq_src = sequencing::none();
            let driver = Driver::from_config(config, shutdown_recv, seq_src).await?;

            assert_eq!(
                driver.engine_driver.finalized_head.number,
                finalized_block.number.unwrap().as_u64()
            );
        }
        Ok(())
    }
}
