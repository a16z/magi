use std::sync::{Arc, Mutex};

use eyre::Result;
use tokio::sync::mpsc::Receiver;

use crate::{config::Config, engine::PayloadAttributes};

use self::{
    stages::{
        attributes::Attributes, batcher_transactions::BatcherTransactions, batches::Batches,
        channels::Channels,
    },
    state::State,
};

pub mod stages;
pub mod state;

pub struct Pipeline {
    attributes: Attributes,
}

impl Iterator for Pipeline {
    type Item = PayloadAttributes;

    fn next(&mut self) -> Option<Self::Item> {
        self.attributes.next()
    }
}

impl Pipeline {
    pub fn new(
        state: Arc<Mutex<State>>,
        tx_recv: Receiver<Vec<u8>>,
        config: Arc<Config>,
    ) -> Result<Self> {
        let batcher_transactions = BatcherTransactions::new(tx_recv);
        let channels = Channels::new(batcher_transactions, config.clone());
        let batches = Batches::new(channels, state.clone(), config.clone());
        let attributes = Attributes::new(batches, config, state);

        Ok(Self { attributes })
    }
}
