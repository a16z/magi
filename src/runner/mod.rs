use std::{
    process,
    sync::{
        mpsc::{channel, Receiver},
        Arc,
    },
    time::Duration,
};

use ethers::{
    providers::{Middleware, Provider},
    types::H256,
};
use eyre::Result;
use tokio::time::sleep;

use crate::{
    config::{Config, SyncMode},
    driver::Driver,
    engine::{Engine, EngineApi, ExecutionPayload, ForkchoiceState, Status},
};

const TRUSTED_PEER_ENODE: &str = "enode://e85ba0beec172b17f53b373b0ab72238754259aa39f1ae5290e3244e0120882f4cf95acd203661a27c8618b27ca014d4e193266cb3feae43655ed55358eedb06@3.86.143.120:30303?discport=21693";

pub struct Runner {
    config: Config,
    sync_mode: SyncMode,
    checkpoint_hash: Option<String>,
    shutdown_recv: Arc<Receiver<bool>>,
}

impl Runner {
    pub fn from_config(config: Config) -> Self {
        let (shutdown_sender, shutdown_recv) = channel();
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
            shutdown_recv: Arc::new(shutdown_recv),
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

    pub async fn run(&self) -> Result<()> {
        match self.sync_mode {
            SyncMode::Fast => self.fast_sync().await,
            SyncMode::Checkpoint => self.checkpoint_sync().await,
            SyncMode::Full => self.full_sync().await,
            SyncMode::Challenge => self.challenge_sync().await,
        }
    }

    pub async fn fast_sync(&self) -> Result<()> {
        unimplemented!();
    }

    pub async fn challenge_sync(&self) -> Result<()> {
        unimplemented!();
    }

    pub async fn full_sync(&self) -> Result<()> {
        tracing::info!("starting full sync");

        self.start_driver().await?;
        Ok(())
    }

    pub async fn checkpoint_sync(&self) -> Result<()> {
        tracing::info!("starting checkpoint sync");

        let checkpoint_hash = match self.checkpoint_hash {
            Some(ref checkpoint) => checkpoint
                .parse()
                .expect("invalid checkpoint block hash provided"),
            None => {
                tracing::info!("fetching latest checkpoint from the L2 chain");

                // TODO: get epoch boundary L2 block hash nearest to the finalized head
                H256::zero()
            }
        };

        let provider = Provider::try_from(&self.config.l2_rpc_url)?;
        let engine_api = EngineApi::new(&self.config.l2_engine_url, &self.config.jwt_secret);

        while !engine_api.is_available().await {
            if let Ok(shutdown) = self.shutdown_recv.try_recv() {
                if shutdown {
                    process::exit(0);
                }
            }
            sleep(Duration::from_secs(3)).await;
        }

        // if the checkpoint block is already synced, start from the finalized head
        if provider.get_block(checkpoint_hash).await?.is_some() {
            tracing::warn!("finalized head is above the checkpoint block");
            self.start_driver().await?;
            return Ok(());
        }

        // this is a temporary fix to allow execution layer peering to work
        // TODO: use a list of whitelisted bootnodes instead
        provider.add_peer(TRUSTED_PEER_ENODE.to_string()).await?;

        // build the execution payload from the checkpoint block and send it to the execution client
        // (this requires a trusted L2 rpc url)
        let checkpoint_payload =
            ExecutionPayload::from_block_hash(&self.config, checkpoint_hash).await?;
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

        tracing::info!(
            "syncing execution client to the checkpoint block. This could take a few hours",
        );

        loop {
            if let Ok(shutdown) = self.shutdown_recv.try_recv() {
                if shutdown {
                    process::exit(0);
                }
            }

            if provider.get_block_number().await? < checkpoint_payload.block_number {
                sleep(Duration::from_secs(3)).await;
            } else {
                break;
            }
        }

        tracing::info!("execution client successfully synced to the checkpoint block");
        Driver::from_config(self.config.clone(), self.shutdown_recv.clone()).await?;
        Ok(())
    }

    async fn start_driver(&self) -> Result<()> {
        let mut driver =
            Driver::from_config(self.config.clone(), self.shutdown_recv.clone()).await?;

        if let Err(err) = driver.start().await {
            tracing::error!("driver failure: {}", err);
            std::process::exit(1);
        }

        Ok(())
    }
}
