use std::{cell::RefCell, collections::HashMap, rc::Rc};

use ethers::types::{Block, Transaction, H256};
use eyre::Result;

use magi::{
    base_chain::chain_watcher,
    stages::{
        attributes::Attributes, batcher_transactions::BatcherTransactions, batches::Batches,
        channels::Channels, Stage,
    },
};

#[tokio::main]
async fn main() -> Result<()> {
    stages().await?;

    Ok(())
}

pub async fn stages() -> Result<()> {
    let start_epoch = 8494058;
    let (mut batcher_tx_recv, mut block_recv) = chain_watcher(start_epoch);

    let blocks = Rc::new(RefCell::new(HashMap::<H256, Block<Transaction>>::new()));

    let batcher_txs = BatcherTransactions::new();
    let channels = Channels::new(batcher_txs.clone());
    let batches = Batches::new(channels.clone(), start_epoch);
    let attributes = Attributes::new(batches.clone(), blocks.clone());

    while let Some(data) = batcher_tx_recv.recv().await {
        while let Ok(block) = block_recv.try_recv() {
            blocks.borrow_mut().insert(block.hash.unwrap(), block);
        }

        batcher_txs.borrow_mut().push_data(data)?;

        while let Some(payload_attributes) = attributes.borrow_mut().next()? {
            println!("{:?}", payload_attributes);
            break;
        }
    }

    Ok(())
}
