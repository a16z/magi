use std::{str::FromStr, time::Duration};

use ethers::{
    providers::{Middleware, Provider},
    types::Address,
};
use tokio::{
    spawn,
    sync::mpsc::{channel, Receiver},
    time::sleep,
};

type BatcherTransactionData = Vec<u8>;

pub fn chain_watcher(start_block: u64) -> Receiver<BatcherTransactionData> {
    let (sender, receiver) = channel(100);

    spawn(async move {
        let url = "https://eth-goerli.g.alchemy.com/v2/a--NIcyeycPntQX42kunxUIVkg6_ekYc";
        let provider = Provider::try_from(url).unwrap();

        let batch_sender = Address::from_str("0x7431310e026b69bfc676c0013e12a1a11411eec9").unwrap();
        let batch_inbox = Address::from_str("0xff00000000000000000000000000000000000420").unwrap();

        let mut block_num = start_block;

        loop {
            let block = provider
                .get_block_with_txs(block_num)
                .await
                .unwrap()
                .unwrap();
            let mut batcher_txs = block.transactions.into_iter().filter(|tx| {
                tx.from == batch_sender && tx.to.map(|to| to == batch_inbox).unwrap_or(false)
            });

            while let Some(batcher_tx) = batcher_txs.next() {
                sender.send(batcher_tx.input.to_vec()).await.unwrap();
            }

            block_num += 1;

            sleep(Duration::from_millis(250)).await;
        }
    });

    receiver
}
