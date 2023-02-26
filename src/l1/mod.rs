use std::{sync::Arc, time::Duration};

use ethers_core::{
    types::{Block, Filter, Transaction, H256},
    utils::keccak256,
};
use ethers_providers::{Middleware, Provider};

use tokio::{
    spawn,
    sync::mpsc::{channel, Receiver},
    task::JoinHandle,
    time::sleep,
};

use crate::{config::Config, derive::stages::attributes::UserDeposited};

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
    pub fn new(start_block: u64, config: Arc<Config>) -> Self {
        let (handle, tx_receiver, block_receiver, deposit_receiver) =
            chain_watcher(start_block, config);
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
    config: Arc<Config>,
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
        let rpc_url = config.l1_rpc.clone();
        let provider = Provider::try_from(rpc_url).unwrap();

        let batch_sender = config.chain.batch_sender;
        let batch_inbox = config.chain.batch_inbox;
        let deposit_contract = config.chain.deposit_contract;

        let deposit_event = "TransactionDeposited(address,address,uint256,bytes)";
        let deposit_topic = H256::from_slice(&keccak256(deposit_event));

        let deposit_filter = Filter::new()
            .address(deposit_contract)
            .topic0(deposit_topic);

        let mut block_num = start_block;

        loop {
            tracing::debug!("fetching l1 data for block {}", block_num);

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
