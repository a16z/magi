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
    types::{BlockNumber, H256},
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

        let l1_provider = Provider::try_from(&self.config.l1_rpc_url)?;
        let l2_provider = Provider::try_from(&self.config.l2_rpc_url)?;
        let l2_trusted_provider = Provider::try_from(
            &self
                .config
                .l2_trusted_rpc_url
                .clone()
                .expect("a trusted L2 rpc url is required for checkpoint sync"),
        )?;

        let checkpoint_hash = match self.checkpoint_hash {
            Some(ref checkpoint) => checkpoint
                .parse()
                .expect("invalid checkpoint block hash provided"),
            None => {
                tracing::info!("using the latest epoch boundary as checkpoint block");

                let mut block_number = BlockNumber::Latest;
                loop {
                    self.check_shutdown()?;

                    let l2_block = l2_trusted_provider
                        .get_block_with_txs(block_number)
                        .await?
                        .unwrap_or_default();

                    let set_l1_block_values_tx = l2_block
                        .transactions
                        .iter()
                        .find(|tx| tx.to.unwrap() == self.config.chain.l1_block)
                        .expect("could not find setL1BlockValues tx in the fetched block");

                    let sequence_number = &set_l1_block_values_tx
                        .clone()
                        .input
                        .into_iter()
                        .skip(132)
                        .take(32)
                        .collect::<Vec<u8>>();

                    if H256::from_slice(sequence_number) != H256::zero() {
                        block_number = BlockNumber::Number(
                            l2_block.number.expect("fetched block has no number") - 1,
                        );
                        continue;
                    }

                    break l2_block.hash.expect("fetched block has no hash");
                }
            }
        };

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
        let checkpoint_payload = ExecutionPayload::from_block_hash(
            &self.config,
            checkpoint_hash,
            l1_provider,
            l2_trusted_provider,
        )
        .await?;

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

        if let Err(err) = driver.start().await {
            tracing::error!("driver failure: {}", err);
            std::process::exit(1);
        }

        Ok(())
    }

    fn check_shutdown(&self) -> Result<()> {
        if let Ok(shutdown) = self.shutdown_recv.try_recv() {
            if shutdown {
                tracing::warn!("shutting down");
                process::exit(0);
            }
        }

        Ok(())
    }
}
