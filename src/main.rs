use std::str::FromStr;

use ethers::{
    providers::{Middleware, Provider},
    types::H256,
};
use eyre::Result;

use magi::stages::{
    batcher_transactions::BatcherTransactions, batches::Batches, channels::Channels, Stage,
};

#[tokio::main]
async fn main() -> Result<()> {
    let provider =
        Provider::try_from("https://eth-goerli.g.alchemy.com/v2/a--NIcyeycPntQX42kunxUIVkg6_ekYc")?;

    let tx_hash =
        H256::from_str("0xb6ee418c161c1daee9f504c34209a6d3ef1d65e4aba35c1ffa90579c9d57c963")?;

    let tx = provider.get_transaction(tx_hash).await?.unwrap();
    let data = tx.input.to_vec();

    let batchers_txs = BatcherTransactions::new();
    let channels = Channels::new(batchers_txs.clone());
    let batches = Batches::new(channels.clone());

    batchers_txs.borrow_mut().push_data(data)?;

    while let Some(batch) = batches.borrow_mut().next()? {
        println!("{:?}", batch);
    }

    Ok(())
}
