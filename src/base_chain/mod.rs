use std::{str::FromStr, time::Duration};

use ethers::{
    providers::{Middleware, Provider},
    types::{Address, Block, Filter, Transaction, H256},
};
use tokio::{
    spawn,
    sync::mpsc::{channel, Receiver},
    task::JoinHandle,
    time::sleep,
};

use crate::derive::stages::attributes::UserDeposited;

pub struct ChainWatcher {
    handle: JoinHandle<()>,
    pub tx_receiver: Option<Receiver<BatcherTransactionData>>,
    pub block_receiver: Receiver<Block<Transaction>>,
    pub deposit_receiver: Receiver<UserDeposited>,
}

impl Drop for ChainWatcher {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

impl ChainWatcher {
    pub fn new(start_block: u64) -> Self {
        let (handle, tx_receiver, block_receiver, deposit_receiver) = chain_watcher(start_block);
        Self {
            handle,
            tx_receiver: Some(tx_receiver),
            block_receiver,
            deposit_receiver,
        }
    }

    pub fn take_tx_receiver(&mut self) -> Option<Receiver<BatcherTransactionData>> {
        self.tx_receiver.take()
    }
}

fn chain_watcher(
    start_block: u64,
) -> (
    JoinHandle<()>,
    Receiver<BatcherTransactionData>,
    Receiver<Block<Transaction>>,
    Receiver<UserDeposited>,
) {
    let (batcher_tx_sender, batcher_tx_receiver) = channel(1000);
    let (block_sender, block_receiver) = channel(1000);
    let (deposit_sender, deposit_receiver) = channel(1000);

    let handle = spawn(async move {
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
            let channel_full = batcher_tx_sender.capacity() == 0
                || block_sender.capacity() == 0
                || deposit_sender.capacity() == 0;

            if channel_full {
                sleep(Duration::from_millis(250)).await;
                continue;
            }

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
        }
    });

    (
        handle,
        batcher_tx_receiver,
        block_receiver,
        deposit_receiver,
    )
}

type BatcherTransactionData = Vec<u8>;
