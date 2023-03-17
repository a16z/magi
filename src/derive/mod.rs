use std::sync::{Arc, Mutex, RwLock};

use eyre::Result;

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
    batcher_transactions: Arc<Mutex<BatcherTransactions>>,
    channels: Arc<Mutex<Channels>>,
    batches: Arc<Mutex<Batches>>,
    attributes: Attributes,
    pending_attributes: Option<PayloadAttributes>,
}

impl Iterator for Pipeline {
    type Item = PayloadAttributes;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pending_attributes.is_some() {
            self.pending_attributes.take()
        } else {
            self.attributes.next()
        }
    }
}

impl Pipeline {
    pub fn new(state: Arc<RwLock<State>>, config: Arc<Config>) -> Result<Self> {
        let batcher_transactions = BatcherTransactions::new();
        let channels = Channels::new(batcher_transactions.clone(), config.clone());
        let batches = Batches::new(channels.clone(), state.clone(), config);
        let attributes = Attributes::new(batches.clone(), state);

        Ok(Self {
            batcher_transactions,
            channels,
            batches,
            attributes,
            pending_attributes: None,
        })
    }

    pub fn push_batcher_transactions(&self, txs: Vec<Vec<u8>>, l1_origin: u64) -> Result<()> {
        self.batcher_transactions
            .lock()
            .map_err(|_| eyre::eyre!("lock poisoned"))?
            .push_data(txs, l1_origin);

        Ok(())
    }

    pub fn peak(&mut self) -> Option<&PayloadAttributes> {
        if self.pending_attributes.is_none() {
            let next_attributes = self.next();
            self.pending_attributes = next_attributes;
        }

        self.pending_attributes.as_ref()
    }

    pub fn purge(&mut self) -> Result<()> {
        self.batcher_transactions
            .lock()
            .map_err(|_| eyre::eyre!("lock poisoned"))?
            .purge();
        self.channels
            .lock()
            .map_err(|_| eyre::eyre!("lock poisoned"))?
            .purge();
        self.batches
            .lock()
            .map_err(|_| eyre::eyre!("lock poisoned"))?
            .purge();
        self.attributes.purge();

        Ok(())
    }
}
