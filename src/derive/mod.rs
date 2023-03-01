use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use ethers_core::types::{Block, Transaction, H256};
use eyre::Result;

use crate::{config::Config, engine::PayloadAttributes, l1::ChainWatcher};

use self::stages::{
    attributes::{Attributes, UserDeposited},
    batcher_transactions::BatcherTransactions,
    batches::Batches,
    channels::Channels,
};

pub mod stages;

pub struct Pipeline {
    attributes: Rc<RefCell<Attributes>>,
    chain_watcher: ChainWatcher,
    blocks: Rc<RefCell<HashMap<H256, Block<Transaction>>>>,
    deposits: Rc<RefCell<HashMap<u64, Vec<UserDeposited>>>>,
}

impl Iterator for Pipeline {
    type Item = PayloadAttributes;

    fn next(&mut self) -> Option<Self::Item> {
        self.update_blocks();
        self.update_deposits();
        self.attributes.borrow_mut().next()
    }
}

impl Pipeline {
    pub fn new(start_epoch: u64, config: Arc<Config>) -> Result<Self> {
        let mut chain_watcher = ChainWatcher::new(start_epoch, config.clone())?;
        let tx_recv = chain_watcher
            .take_tx_receiver()
            .ok_or(eyre::eyre!("tx receiver already taken"))?;

        let blocks = Rc::new(RefCell::new(HashMap::<H256, Block<Transaction>>::new()));
        let deposits = Rc::new(RefCell::new(HashMap::<u64, Vec<UserDeposited>>::new()));

        let batcher_transactions = BatcherTransactions::new(tx_recv);
        let channels = Channels::new(batcher_transactions, Arc::clone(&config));
        let batches = Batches::new(channels, start_epoch);
        let attributes = Attributes::new(batches, config, blocks.clone(), deposits.clone());

        Ok(Self {
            attributes,
            chain_watcher,
            blocks,
            deposits,
        })
    }

    fn update_blocks(&mut self) {
        while let Ok(block) = self.chain_watcher.block_receiver.try_recv() {
            match block.hash {
                Some(hash) => {
                    self.blocks.borrow_mut().insert(hash, block);
                }
                None => {
                    tracing::warn!("chain watcher found block #{:?} without hash", block.number)
                }
            }
        }
    }

    fn update_deposits(&mut self) {
        while let Ok(deposit) = self.chain_watcher.deposit_receiver.try_recv() {
            let mut deposits = self.deposits.borrow_mut();
            let deposits_for_block = deposits.get_mut(&deposit.l1_block_num);

            if let Some(deposits_for_block) = deposits_for_block {
                deposits_for_block.push(deposit);
            } else {
                deposits.insert(deposit.l1_block_num, vec![deposit]);
            }
        }
    }
}
