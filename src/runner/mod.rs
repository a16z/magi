use std::{
    process,
    sync::{Arc, RwLock},
    time::Duration,
};

use ethers::{
    providers::{Http, Middleware, Provider},
    types::{BlockId, BlockNumber, H256},
};
use eyre::Result;
use tokio::{
    sync::{
        watch::{channel, Receiver},
        RwLock as TokioRwLock,
    },
    time::sleep,
};

use crate::{
    config::{Config, SyncMode, SystemAccounts},
    derive::state::State,
    driver::{
        engine_driver::EngineDriver,
        sequencing::{self, driver::SequencingDriver},
        Driver,
    },
    engine::{Engine, EngineApi, ExecutionPayload, ForkchoiceState, Status},
    specular,
};

const TRUSTED_PEER_ENODE: &str = "enode://e85ba0beec172b17f53b373b0ab72238754259aa39f1ae5290e3244e0120882f4cf95acd203661a27c8618b27ca014d4e193266cb3feae43655ed55358eedb06@3.86.143.120:30303?discport=21693";

pub struct Runner {
    config: Config,
    sync_mode: SyncMode,
    checkpoint_hash: Option<String>,
    shutdown_recv: Receiver<bool>,
}

impl Runner {
    pub fn from_config(config: Config) -> Self {
        let (shutdown_sender, shutdown_recv) = channel(false);
        ctrlc::set_handler(move || {
            tracing::info!("shutting down");
            shutdown_sender
                .send(true)
                .expect("could not send shutdown signal");
        })
        .expect("could not register shutdown handler");

        Self {
            config,
            sync_mode: SyncMode::Full,
            checkpoint_hash: None,
            shutdown_recv,
        }
    }

    pub fn with_sync_mode(mut self, sync_mode: SyncMode) -> Self {
        self.sync_mode = sync_mode;
        self
    }

    pub fn with_checkpoint_hash(mut self, checkpoint_hash: Option<String>) -> Self {
        self.checkpoint_hash = checkpoint_hash;
        self
    }

    pub async fn run(self) -> Result<()> {
        match self.sync_mode {
            SyncMode::Fast => self.fast_sync().await,
            SyncMode::Challenge => self.challenge_sync().await,
            SyncMode::Full => self.full_sync().await,
            SyncMode::Checkpoint => self.checkpoint_sync().await,
        }
    }

    pub async fn fast_sync(&self) -> Result<()> {
        tracing::error!("fast sync is not implemented yet");
        unimplemented!();
    }

    pub async fn challenge_sync(&self) -> Result<()> {
        tracing::error!("challenge sync is not implemented yet");
        unimplemented!();
    }

    pub async fn full_sync(&self) -> Result<()> {
        tracing::info!("starting full sync");

        self.start_driver().await?;
        Ok(())
    }

    pub async fn checkpoint_sync(&self) -> Result<()> {
        tracing::info!("starting checkpoint sync");

        let l2_provider = Provider::try_from(&self.config.l2_rpc_url)?;
        let checkpoint_sync_url =
            Provider::try_from(self.config.checkpoint_sync_url.as_ref().ok_or(eyre::eyre!(
                "a checkpoint sync rpc url is required for checkpoint sync"
            ))?)?;

        let checkpoint_block = match self.checkpoint_hash {
            Some(ref checkpoint) => {
                let block_hash: H256 = checkpoint
                    .parse()
                    .expect("invalid checkpoint block hash provided");

                match Self::is_epoch_boundary(block_hash, &checkpoint_sync_url).await? {
                    true => checkpoint_sync_url
                        .get_block_with_txs(block_hash)
                        .await?
                        .expect("could not get checkpoint block"),
                    false => {
                        tracing::error!("the provided checkpoint block is not an epoch boundary");
                        process::exit(1);
                    }
                }
            }
            None => {
                tracing::info!("finding the latest epoch boundary to use as checkpoint");

                let mut block_number = checkpoint_sync_url.get_block_number().await?;
                while !Self::is_epoch_boundary(block_number, &checkpoint_sync_url).await? {
                    self.check_shutdown()?;
                    block_number -= 1.into();
                }

                let block = checkpoint_sync_url
                    .get_block(BlockId::Number(BlockNumber::Number(block_number)))
                    .await?
                    .expect("could not get block");

                checkpoint_sync_url
                    .get_block_with_txs(block.hash.expect("block hash is missing"))
                    .await?
                    .expect("could not get checkpoint block")
            }
        };

        let checkpoint_hash = checkpoint_block.hash.expect("block hash is missing");
        tracing::info!("using checkpoint block {}", checkpoint_hash);

        let engine_api = EngineApi::new(&self.config.l2_engine_url, &self.config.jwt_secret);
        while !engine_api.is_available().await {
            self.check_shutdown()?;
            sleep(Duration::from_secs(3)).await;
        }

        // if the checkpoint block is already synced, start from the finalized head
        if l2_provider.get_block(checkpoint_hash).await?.is_some() {
            tracing::warn!("finalized head is above the checkpoint block");
            self.start_driver().await?;
            return Ok(());
        }

        // this is a temporary fix to allow execution layer peering to work
        // TODO: use a list of whitelisted bootnodes instead
        tracing::info!("adding trusted peer to the execution layer");
        l2_provider.add_peer(TRUSTED_PEER_ENODE.to_string()).await?;

        // build the execution payload from the checkpoint block and send it to the execution client
        let checkpoint_payload = ExecutionPayload::try_from(checkpoint_block)?;

        let payload_res = engine_api.new_payload(checkpoint_payload.clone()).await?;
        if let Status::Invalid | Status::InvalidBlockHash = payload_res.status {
            tracing::error!("the provided checkpoint payload is invalid, exiting");
            process::exit(1);
        }

        // make the execution client start syncing up to the checkpoint
        let forkchoice_state = ForkchoiceState::from_single_head(checkpoint_hash);
        let forkchoice_res = engine_api
            .forkchoice_updated(forkchoice_state, None)
            .await?;
        if let Status::Invalid | Status::InvalidBlockHash = forkchoice_res.payload_status.status {
            tracing::error!("could not accept forkchoice, exiting");
            process::exit(1);
        }

        tracing::info!("syncing execution client to the checkpoint block...",);

        while l2_provider.get_block_number().await? < checkpoint_payload.block_number {
            self.check_shutdown()?;
            sleep(Duration::from_secs(3)).await;
        }

        tracing::info!("execution client successfully synced to the checkpoint block");

        self.start_driver().await?;
        Ok(())
    }

    async fn start_driver(&self) -> Result<()> {
        let mut driver =
            Driver::from_config(self.config.clone(), self.shutdown_recv.clone()).await?;
        let seq_fut = {
            let engine_driver = driver.engine_driver.clone();
            let state = driver.state.clone();
            self.start_sequencing_driver(engine_driver, state)
        };
        let driver_fut = driver.start();
        if let Err(err) = futures::try_join!(driver_fut, seq_fut) {
            tracing::error!("driver failure: {}", err);
            std::process::exit(1);
        }

        Ok(())
    }

    async fn start_sequencing_driver(
        &self,
        engine_driver: Arc<TokioRwLock<EngineDriver<EngineApi>>>,
        state: Arc<RwLock<State>>,
    ) -> Result<()> {
        match (
            self.config.local_sequencer.enabled,
            self.config.chain.meta.enable_full_derivation,
        ) {
            // TODO: use a src that conforms to optimism's full derivation protocol.
            (true, true) => panic!("not currently supported"),
            (true, false) => {
                // Use specular sequencing.
                let mut driver = {
                    let cfg = specular::sequencing::config::Config::new(&self.config);
                    let l2_provider = Provider::try_from(&self.config.l2_rpc_url)?;
                    let policy = specular::sequencing::AttributesBuilder::new(cfg, l2_provider);
                    let l1_provider = Provider::try_from(&self.config.l1_rpc_url)?;
                    let sequencing_src = sequencing::Source::new(policy, l1_provider.clone());
                    SequencingDriver::new(
                        engine_driver,
                        state,
                        sequencing_src,
                        l1_provider,
                        self.shutdown_recv.clone(),
                    )
                };
                driver.start().await
            }
            _ => Ok(()),
        }
    }

    fn check_shutdown(&self) -> Result<()> {
        if *self.shutdown_recv.borrow() {
            tracing::warn!("shutting down");
            process::exit(0);
        }

        Ok(())
    }

    async fn is_epoch_boundary<T: Into<BlockId> + Send + Sync>(
        block: T,
        checkpoint_sync_url: &Provider<Http>,
    ) -> Result<bool> {
        let l2_block = checkpoint_sync_url
            .get_block_with_txs(block)
            .await?
            .ok_or_else(|| eyre::eyre!("could not find block"))?;

        // TODO[zhe]: change to specular variant to support checkpoint sync (or implement outside)
        let sequence_number = &l2_block
            .transactions
            .iter()
            .find(|tx| tx.to.unwrap() == SystemAccounts::default().attributes_predeploy)
            .expect("could not find setL1BlockValues tx in the epoch boundary search")
            .input
            .clone()
            .into_iter()
            .skip(132)
            .take(32)
            .collect::<Vec<u8>>();

        Ok(sequence_number == &[0; 32])
    }
}
