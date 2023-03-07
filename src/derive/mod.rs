use std::{collections::HashMap, sync::{Arc, Mutex}};

use ethers_core::types::H256;
use eyre::Result;

use crate::{
    config::Config,
    engine::PayloadAttributes,
    l1::{ChainWatcher, L1Info},
};

use self::stages::{
    attributes::Attributes, batcher_transactions::BatcherTransactions, batches::Batches,
    channels::Channels,
};

pub mod stages;

pub struct Pipeline {
    attributes: Arc<Mutex<Attributes>>,
    chain_watcher: ChainWatcher,
    l1_info: Arc<Mutex<HashMap<H256, L1Info>>>,
}

impl Iterator for Pipeline {
    type Item = PayloadAttributes;

    fn next(&mut self) -> Option<Self::Item> {
        self.update_l1_info();
        tracing::debug!(target: "magi", "pipeline updated new l1 info");
        self.attributes.lock().ok().and_then(|mut a| a.next())
    }
}

impl Pipeline {
    pub fn new(start_epoch: u64, config: Arc<Config>) -> Result<Self> {
        let mut chain_watcher = ChainWatcher::new(start_epoch, config.clone())?;
        let tx_recv = chain_watcher
            .take_tx_receiver()
            .ok_or(eyre::eyre!("tx receiver already taken"))?;

        let l1_info = Arc::new(Mutex::new(HashMap::<H256, L1Info>::new()));

        let batcher_transactions = BatcherTransactions::new(tx_recv);
        let channels = Channels::new(batcher_transactions, Arc::clone(&config));
        let batches = Batches::new(channels, start_epoch);
        let attributes = Attributes::new(batches, config, l1_info.clone());

        Ok(Self {
            attributes,
            chain_watcher,
            l1_info,
        })
    }

    fn update_l1_info(&mut self) {
        while let Ok(l1_info) = self.chain_watcher.l1_info_receiver.try_recv() {
            self.l1_info
                .lock()
                .unwrap()
                .insert(l1_info.block_info.hash, l1_info);
        }
    }
}
