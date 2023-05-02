use std::sync::mpsc::{channel, Receiver};

use ethers::types::H256;
use eyre::Result;

use crate::{
    config::{Config, SyncMode},
    driver::Driver,
};

pub struct Runner {
    config: Config,
    sync_mode: SyncMode,
    checkpoint_hash: Option<String>,
}

impl Runner {
    pub fn from_config(config: Config) -> Self {
        Self {
            config,
            sync_mode: SyncMode::Full,
            checkpoint_hash: None,
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
        let shutdown_recv = self.shutdown_on_ctrlc();

        let mut driver = Driver::from_config(self.config.clone(), shutdown_recv).await?;

        if let Err(err) = driver.start().await {
            tracing::error!("{}", err);
            std::process::exit(1);
        }

        Ok(())
    }

    pub async fn checkpoint_sync(&self) -> Result<()> {
        tracing::info!("starting checkpoint sync");
        let shutdown_recv = self.shutdown_on_ctrlc();

        let checkpoint_hash = match self.checkpoint_hash {
            Some(ref checkpoint) => checkpoint
                .parse()
                .expect("invalid checkpoint block hash provided"),
            None => {
                tracing::info!(
                    "initializing checkpoint sync without an explicit checkpoint hash. \
                    This will use the latest finalized L2 block as the checkpoint. \
                    To specify a checkpoint hash manually, use the --checkpoint-hash argument."
                );

                // TODO: get epoch boundary L2 block hash nearest to the finalized head
                H256::zero()
            }
        };

        let mut driver =
            Driver::from_checkpoint(self.config.clone(), shutdown_recv, checkpoint_hash).await?;

        if let Err(err) = driver.start().await {
            tracing::error!("{}", err);
            std::process::exit(1);
        }

        Ok(())
    }

    fn shutdown_on_ctrlc(&self) -> Receiver<bool> {
        let (shutdown_sender, shutdown_recv) = channel();
        ctrlc::set_handler(move || {
            tracing::info!("shutting down");
            shutdown_sender
                .send(true)
                .expect("could not send shutdown signal");
        })
        .expect("could not register shutdown handler");

        shutdown_recv
    }
}
