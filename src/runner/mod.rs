use std::{process, time::Duration};

use ethers::{
    providers::{Middleware, Provider},
    types::{Block, BlockId, BlockNumber, Transaction, H256},
};
use eyre::Result;
use tokio::{
    sync::watch::{channel, Receiver},
    time::sleep,
};

use crate::{
    config::{Config, SyncMode},
    driver::{Driver, HeadInfo},
    engine::{Engine, EngineApi, ExecutionPayload, ForkchoiceState, Status},
};

/// Temporary trusted/static peer used for checkpoint sync mode.
// TODO: use a list of whitelisted bootnodes instead
const TRUSTED_PEER_ENODE: &str = "enode://e85ba0beec172b17f53b373b0ab72238754259aa39f1ae5290e3244e0120882f4cf95acd203661a27c8618b27ca014d4e193266cb3feae43655ed55358eedb06@3.86.143.120:30303?discport=21693";

/// The main entrypoint for starting a Magi node.
/// Responsible for starting the syncing process.
pub struct Runner {
    /// The Magi [Config]
    config: Config,
    /// The [SyncMode] - currently full & checkpoint sync are supported
    sync_mode: SyncMode,
    /// The L2 block hash to begin syncing from
    checkpoint_hash: Option<String>,
    /// Receiver to listen for SIGINT signals
    shutdown_recv: Receiver<bool>,
}

impl Runner {
    /// Creates a new [Runner] from a [Config] and registers the SIGINT signal handler.
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

    /// Sets the [SyncMode]
    pub fn with_sync_mode(mut self, sync_mode: SyncMode) -> Self {
        self.sync_mode = sync_mode;
        self
    }

    /// Sets the `checkpoint_hash` if running in checkpoint [SyncMode]
    pub fn with_checkpoint_hash(mut self, checkpoint_hash: Option<String>) -> Self {
        self.checkpoint_hash = checkpoint_hash;
        self
    }

    /// Begins the syncing process
    pub async fn run(self) -> Result<()> {
        match self.sync_mode {
            SyncMode::Fast => self.fast_sync().await,
            SyncMode::Challenge => self.challenge_sync().await,
            SyncMode::Full => self.full_sync().await,
            SyncMode::Checkpoint => self.checkpoint_sync().await,
        }
    }

    /// Fast sync mode - currently unsupported
    pub async fn fast_sync(&self) -> Result<()> {
        tracing::error!("fast sync is not implemented yet");
        unimplemented!();
    }

    /// Fast challenge mode - currently unsupported
    pub async fn challenge_sync(&self) -> Result<()> {
        tracing::error!("challenge sync is not implemented yet");
        unimplemented!();
    }

    /// Full sync mode.
    /// Syncs via L1 block derivation from the latest finalized block the execution client has synced to.
    /// Otherwise syncs from genesis
    pub async fn full_sync(&self) -> Result<()> {
        self.start_driver().await?;
        Ok(())
    }

    /// Checkpoint sync mode.
    /// Syncs the execution client to a given checkpoint block, and then begins the normal derivation sync process via the [Driver]
    ///
    /// Note: the `admin` RPC method must be available on the execution client as checkpoint_sync relies on `admin_addPeer`
    pub async fn checkpoint_sync(&self) -> Result<()> {
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

                let l2_block = checkpoint_sync_url
                    .get_block_with_txs(block_hash)
                    .await?
                    .ok_or_else(|| eyre::eyre!("could not find block"))?;

                match is_epoch_boundary(l2_block, &self.config)? {
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
                let l2_block = checkpoint_sync_url
                    .get_block_with_txs(block_number)
                    .await?
                    .ok_or_else(|| eyre::eyre!("could not find block"))?;

                while !is_epoch_boundary(l2_block.clone(), &self.config)? {
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

    /// Creates and starts the [Driver] which handles the derivation sync process.
    async fn start_driver(&self) -> Result<()> {
        let mut driver =
            Driver::from_config(self.config.clone(), self.shutdown_recv.clone()).await?;

        if let Err(err) = driver.start().await {
            tracing::error!("driver failure: {}", err);
            std::process::exit(1);
        }

        Ok(())
    }

    /// Exits if a SIGINT signal is received
    fn check_shutdown(&self) -> Result<()> {
        if *self.shutdown_recv.borrow() {
            tracing::warn!("shutting down");
            process::exit(0);
        }

        Ok(())
    }
}

/// Returns `true` if the L2 block is the first in an epoch (sequence number 0)
fn is_epoch_boundary(l2_block: Block<Transaction>, config: &Config) -> Result<bool> {
    let head_info = HeadInfo::try_from_l2_block(config, l2_block)?;
    let sequence_number = head_info.sequence_number;

    Ok(sequence_number == 0)
}
