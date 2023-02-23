use std::{str::FromStr, time::Duration};

use ethers::{
    providers::{Middleware, Provider},
    types::{Address, Block, Filter, Transaction, H256},
};
use tokio::{
    spawn,
    sync::mpsc::{channel, Receiver},
    time::sleep,
};

use crate::stages::attributes::UserDeposited;

type BatcherTransactionData = Vec<u8>;

pub fn chain_watcher(
    start_block: u64,
) -> (
    Receiver<BatcherTransactionData>,
    Receiver<Block<Transaction>>,
    Receiver<UserDeposited>,
) {
    let (batcher_tx_sender, batcher_tx_receiver) = channel(1000);
    let (block_sender, block_receiver) = channel(1000);
    let (deposit_sender, deposit_receiver) = channel(1000);

    spawn(async move {
        let url = "https://eth-goerli.g.alchemy.com/v2/a--NIcyeycPntQX42kunxUIVkg6_ekYc";
        let provider = Provider::try_from(url).unwrap();

        let batch_sender = Address::from_str("0x7431310e026b69bfc676c0013e12a1a11411eec9").unwrap();
        let batch_inbox = Address::from_str("0xff00000000000000000000000000000000000420").unwrap();

        let deposit_contract =
            Address::from_str("0x5b47E1A08Ea6d985D6649300584e6722Ec4B1383").unwrap();
        let deposit_topic =
            H256::from_str("0xb3813568d9991fc951961fcb4c784893574240a28925604d09fc577c55bb7c32")
                .unwrap();

        let deposit_filter = Filter::new()
            .address(deposit_contract)
            .topic0(deposit_topic);

        let mut block_num = start_block;

        loop {
            let block = provider
                .get_block_with_txs(block_num)
                .await
                .unwrap()
                .unwrap();

            let batcher_txs = block.transactions.clone().into_iter().filter(|tx| {
                tx.from == batch_sender && tx.to.map(|to| to == batch_inbox).unwrap_or(false)
            });

            let block_hash = block.hash.unwrap();

            // blocks must be sent first to prevent stage from executing on a
            // batch that we do not have the block for yet
            block_sender.send(block).await.unwrap();

            let filter = deposit_filter
                .clone()
                .from_block(block_num)
                .to_block(block_num);

            let deposit_logs = provider.get_logs(&filter).await.unwrap();

            for deposit_log in deposit_logs {
                let deposit = UserDeposited::from_log(deposit_log, block_num, block_hash).unwrap();
                deposit_sender.send(deposit).await.unwrap();
            }

            for batcher_tx in batcher_txs {
                batcher_tx_sender
                    .send(batcher_tx.input.to_vec())
                    .await
                    .unwrap();
            }

            block_num += 1;

            sleep(Duration::from_millis(250)).await;
        }
    });

    (batcher_tx_receiver, block_receiver, deposit_receiver)
}
