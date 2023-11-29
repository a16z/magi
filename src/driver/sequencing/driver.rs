use std::{
    process,
    sync::{Arc, RwLock},
    time::Duration,
};

use eyre::Result;
use tokio::{
    sync::{watch, RwLock as TokioRwLock},
    time::sleep,
};

use crate::{
    derive::state::State,
    driver::engine_driver::{execute_action, ChainHeadType, EngineDriver},
    engine::{Engine, EngineApi},
};

use super::SequencingSource;

pub struct SequencingDriver<E: Engine, S: SequencingSource> {
    /// The engine driver
    engine_driver: Arc<TokioRwLock<EngineDriver<E>>>,
    /// State struct to keep track of global state
    state: Arc<RwLock<State>>,
    /// Local sequencing source
    sequencing_src: S,
    /// Channel to receive the shutdown signal from
    shutdown_recv: watch::Receiver<bool>,
}

impl<S: SequencingSource> SequencingDriver<EngineApi, S> {
    pub fn new(
        engine_driver: Arc<TokioRwLock<EngineDriver<EngineApi>>>,
        state: Arc<RwLock<State>>,
        sequencing_src: S,
        shutdown_recv: watch::Receiver<bool>,
    ) -> SequencingDriver<EngineApi, S> {
        SequencingDriver {
            engine_driver,
            state,
            sequencing_src,
            shutdown_recv,
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

    /// Runs the driver
    pub async fn start(&mut self) -> Result<()> {
        tracing::info!("starting sequencing driver; waiting for engine...");
        self.await_engine_ready().await;
        loop {
            self.check_shutdown().await;
            if let Err(err) = self.advance().await {
                tracing::error!("fatal error: {:?}", err);
                self.shutdown().await;
            }
        }
    }

    /// Attempts to advance sequencing forward using attrs received from `sequencing_src`.
    async fn advance(&mut self) -> Result<()> {
        let step = {
            let engine_driver = self.engine_driver.read().await;
            // Get next attributes from sequencing source.
            let attrs = self
                .sequencing_src
                .get_next_attributes(
                    &self.state,
                    &engine_driver.unsafe_head,
                    &engine_driver.unsafe_epoch,
                )
                .await?;
            // Determine action to take on the next attributes.
            match attrs {
                Some(attrs) => Some((attrs.clone(), engine_driver.determine_action(&attrs).await?)),
                None => None,
            }
        };
        match step {
            Some((attrs, action)) => {
                execute_action(
                    &attrs,
                    action,
                    ChainHeadType::Unsafe,
                    self.engine_driver.clone(),
                )
                .await
            }
            None => {
                tracing::info!("no payload to build");
                Ok(())
            }
        }
    }

    async fn await_engine_ready(&self) {
        while !self.engine_driver.read().await.engine_ready().await {
            self.check_shutdown().await;
            sleep(Duration::from_secs(1)).await;
        }
    }
}
