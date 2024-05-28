//! Module handles running the Magi node.

use std::{process, str::FromStr, time::Duration};

use alloy_primitives::B256;
use alloy_provider::ext::AdminApi;
use alloy_provider::{Provider, ProviderBuilder, ReqwestProvider};
use alloy_rpc_types::{BlockId, BlockNumberOrTag, BlockTransactions, RpcBlockHash};

use anyhow::Result;
use tokio::{
    sync::watch::{channel, Receiver},
    time::sleep,
};

use crate::{
    config::{Config, SyncMode, SystemAccounts},
    driver::NodeDriver,
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
        let l2_rpc_url = reqwest::Url::parse(&self.config.l2_rpc_url)
            .map_err(|err| anyhow::anyhow!(format!("unable to parse l2_rpc_url: {err}")))?;
        let l2_provider = ProviderBuilder::new().on_http(l2_rpc_url);

        let checkpoint_sync_url = self.config.checkpoint_sync_url.as_ref().ok_or(anyhow::anyhow!(
            "checkpoint_sync_url is required for checkpoint sync mode"
        ))?;
        let checkpoint_sync_url = reqwest::Url::parse(checkpoint_sync_url)
            .map_err(|err| anyhow::anyhow!(format!("unable to parse checkpoint_sync_url: {err}")))?;
        let checkpoint_sync_provider = ProviderBuilder::new().on_http(checkpoint_sync_url);

        let checkpoint_block = match self.checkpoint_hash {
            Some(ref checkpoint) => {
                let hash = B256::from_str(checkpoint)
                    .map_err(|_| anyhow::anyhow!("invalid checkpoint block hash provided"))?;
                let block_hash = RpcBlockHash::from_hash(hash, None);
                let block_id = BlockId::Hash(block_hash);

                match Self::is_epoch_boundary(block_id, &checkpoint_sync_provider).await? {
                    true => checkpoint_sync_provider
                        .get_block(BlockId::Hash(block_hash), true)
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

                let mut block_number = checkpoint_sync_provider.get_block_number().await?;
                while !Self::is_epoch_boundary(block_number, &checkpoint_sync_provider).await? {
                    self.check_shutdown()?;
                    block_number -= 1u64;
                }

                let block = checkpoint_sync_provider
                    .get_block(
                        BlockId::Number(BlockNumberOrTag::Number(block_number)),
                        false,
                    )
                    .await?
                    .expect("could not get block");

                let block_hash = block.header.hash.expect("block hash is missing");
                let block_hash = BlockId::Hash(RpcBlockHash::from_hash(block_hash, None));
                checkpoint_sync_provider
                    .get_block(block_hash, true)
                    .await?
                    .expect("could not get checkpoint block")
            }
        };

        let checkpoint_hash = checkpoint_block.header.hash.expect("block hash is missing");
        tracing::info!("using checkpoint block {}", checkpoint_hash);

        let engine_api = EngineApi::new(&self.config.l2_engine_url, &self.config.jwt_secret);
        while !engine_api.is_available().await {
            self.check_shutdown()?;
            sleep(Duration::from_secs(3)).await;
        }

        // if the checkpoint block is already synced, start from the finalized head
        let block_hash = BlockId::Hash(RpcBlockHash::from_hash(checkpoint_hash, None));
        if l2_provider.get_block(block_hash, false).await?.is_some() {
            tracing::warn!("finalized head is above the checkpoint block");
            self.start_driver().await?;
            return Ok(());
        }

        // this is a temporary fix to allow execution layer peering to work
        // TODO: use a list of whitelisted bootnodes instead
        tracing::info!("adding trusted peer to the execution layer");
        l2_provider.add_peer(TRUSTED_PEER_ENODE).await?;

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

        let checkpoint_block_num: u64 = checkpoint_payload
            .block_number
            .try_into()
            .map_err(|_| anyhow::anyhow!("could not convert checkpoint block number to u64"))?;
        while l2_provider.get_block_number().await? < checkpoint_block_num {
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
            NodeDriver::from_config(self.config.clone(), self.shutdown_recv.clone()).await?;

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

    /// Returns `true` if the L2 block is the first in an epoch (sequence number 0)
    async fn is_epoch_boundary<T: Into<BlockId> + Send + Sync>(
        block: T,
        checkpoint_sync_provider: &ReqwestProvider,
    ) -> Result<bool> {
        let l2_block = checkpoint_sync_provider
            .get_block(block.into(), true)
            .await?
            .ok_or_else(|| anyhow::anyhow!("could not find block"))?;

        let predeploy = SystemAccounts::default().attributes_predeploy;
        let full_txs = if let BlockTransactions::Full(txs) = l2_block.transactions {
            txs
        } else {
            tracing::error!("could not get full transactions from block");
            return Ok(false);
        };
        let sequence_number = &full_txs
            .iter()
            .find(|tx| tx.to.map_or(false, |to| to == predeploy))
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
