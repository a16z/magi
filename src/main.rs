use std::str::FromStr;

use ethers::{
    providers::{Middleware, Provider},
    types::H256,
};
use eyre::Result;

use op_node_rs::{
    batch_decoder::decode_batches, batcher_transaction::BatcherTransaction,
    channel_bank::ChannelBank,
};

#[tokio::main]
async fn main() -> Result<()> {
    let provider =
        Provider::try_from("https://eth-goerli.g.alchemy.com/v2/a--NIcyeycPntQX42kunxUIVkg6_ekYc")?;

    let tx_hash =
        H256::from_str("0xb6ee418c161c1daee9f504c34209a6d3ef1d65e4aba35c1ffa90579c9d57c963")?;

    let tx = provider.get_transaction(tx_hash).await?.unwrap();
    let data = tx.input.to_vec();

    let batch_tx = BatcherTransaction::from_data(&data)?;

    let mut channel_bank = ChannelBank::new();
    batch_tx
        .frames
        .into_iter()
        .for_each(|f| channel_bank.push_frame(f));

    let channel = channel_bank.next().unwrap();

    let batches = decode_batches(&channel)?;

    println!("{:?}", batches);

    Ok(())
}
