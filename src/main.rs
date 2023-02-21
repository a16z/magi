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
    let mut recv = chain_watcher(8494062);

    let batcher_txs = BatcherTransactions::new();
    let channels = Channels::new(batcher_txs.clone());
    let batches = Batches::new(channels.clone());
    let attributes = Attributes::new(batches.clone());

    while let Some(data) = recv.recv().await {
        batcher_txs.borrow_mut().push_data(data)?;

        while let Some(payload_attributes) = attributes.borrow_mut().next()? {
            println!("{:?}", payload_attributes);
        }
    }

    Ok(())
}
