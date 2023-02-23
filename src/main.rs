use std::{cell::RefCell, collections::HashMap, rc::Rc};

use ethers::types::{Block, Transaction, H256};
use eyre::Result;

use magi::{
    base_chain::ChainWatcher,
    stages::{
        attributes::{Attributes, UserDeposited},
        batcher_transactions::BatcherTransactions,
        batches::Batches,
        channels::Channels,
        Stage,
    },
};

#[tokio::main]
async fn main() -> Result<()> {
    stages().await?;

    Ok(())
}

pub async fn stages() -> Result<()> {
    let start_epoch = 8494058;
    let mut chain_watcher = ChainWatcher::new(start_epoch);

    let blocks = Rc::new(RefCell::new(HashMap::<H256, Block<Transaction>>::new()));
    let deposits = Rc::new(RefCell::new(HashMap::<u64, Vec<UserDeposited>>::new()));

    let batcher_txs = BatcherTransactions::new();
    let channels = Channels::new(batcher_txs.clone());
    let batches = Batches::new(channels.clone(), start_epoch);
    let attributes = Attributes::new(batches.clone(), blocks.clone(), deposits.clone());

    while let Some(data) = chain_watcher.tx_receiver.recv().await {
        while let Ok(block) = chain_watcher.block_receiver.try_recv() {
            blocks.borrow_mut().insert(block.hash.unwrap(), block);
        }

        while let Ok(deposit) = chain_watcher.deposit_receiver.try_recv() {
            let mut deposits = deposits.borrow_mut();
            let deposits_for_block = deposits.get_mut(&deposit.base_block_num);

            if let Some(deposits_for_block) = deposits_for_block {
                deposits_for_block.push(deposit);
            } else {
                deposits.insert(deposit.base_block_num, vec![deposit]);
            }
        }

        batcher_txs.borrow_mut().push_data(data)?;

        while let Some(payload_attributes) = attributes.borrow_mut().next()? {
            println!("{:?}", payload_attributes);
        }
    }

    Ok(())
}
