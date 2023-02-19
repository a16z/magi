use std::{future::pending, str::FromStr};

use ethers::{
    providers::{Middleware, Provider},
    types::H256,
};
use eyre::Result;

use magi::{
    base_chain::chain_watcher,
    stages::{
        batcher_transactions::BatcherTransactions, batches::Batches, channels::Channels, Stage,
    },
};

#[tokio::main]
async fn main() -> Result<()> {
    let mut recv = chain_watcher(8494062);

    let batchers_txs = BatcherTransactions::new();
    let channels = Channels::new(batchers_txs.clone());
    let batches = Batches::new(channels.clone());

    while let Some(data) = recv.recv().await {
        batchers_txs.borrow_mut().push_data(data)?;

        while let Some(batch) = batches.borrow_mut().next()? {
            println!("{:?}", batch);
        }
    }

    Ok(())
}
