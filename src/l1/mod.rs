use std::{str::FromStr, sync::Arc, time::Duration};

use ethers_core::{
    types::{Block, Filter, Transaction, H256},
    utils::keccak256,
};
use ethers_providers::{Http, HttpRateLimitRetryPolicy, Middleware, Provider, RetryClient};

use eyre::Result;
use tokio::{
    spawn,
    sync::mpsc::{channel, Receiver, Sender},
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

struct InnerWatcher {
    config: Arc<Config>,
    provider: Provider<RetryClient<Http>>,
    tx_sender: Sender<BatcherTransactionData>,
    block_sender: Sender<Block<Transaction>>,
    deposit_sender: Sender<UserDeposited>,
}

impl Drop for ChainWatcher {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

impl ChainWatcher {
    pub fn new(start_block: u64, config: Arc<Config>) -> Result<Self> {
        let (handle, receivers) = start_watcher(start_block, config)?;

        Ok(Self {
            handle,
            tx_receiver: Some(receivers.tx_receiver),
            block_receiver: receivers.block_receiver,
            deposit_receiver: receivers.deposit_receiver,
        })
    }

    pub fn take_tx_receiver(&mut self) -> Option<Receiver<BatcherTransactionData>> {
        self.tx_receiver.take()
    }
}

impl InnerWatcher {
    fn new(
        config: Arc<Config>,
        tx_sender: Sender<BatcherTransactionData>,
        block_sender: Sender<Block<Transaction>>,
        deposit_sender: Sender<UserDeposited>,
    ) -> Result<Self> {
        let http = Http::from_str(&config.l1_rpc).map_err(|_| eyre::eyre!("invalid L1 RPC URL"))?;
        let policy = Box::new(HttpRateLimitRetryPolicy);
        let client = RetryClient::new(http, policy, 100, 50);
        let provider = Provider::new(client);

        Ok(Self {
            config,
            provider,
            tx_sender,
            block_sender,
            deposit_sender,
        })
    }

    async fn try_ingest_block(&self, block_num: u64) -> Result<()> {
        if !self.channels_full() {
            let block = self.get_block(block_num).await?;
            let deposits = self.get_deposits(block_num, block.hash.unwrap()).await?;
            let batcher_transactions = self.get_batcher_transactions(&block);

            self.block_sender.send(block).await?;

            for deposit in deposits {
                self.deposit_sender.send(deposit).await?;
            }

            for tx in batcher_transactions {
                self.tx_sender.send(tx).await?;
            }
        } else {
            sleep(Duration::from_millis(250)).await;
        }

        Ok(())
    }

    async fn get_block(&self, block_num: u64) -> Result<Block<Transaction>> {
        self.provider
            .get_block_with_txs(block_num)
            .await?
            .ok_or(eyre::eyre!("block not found"))
    }

    fn get_batcher_transactions(&self, block: &Block<Transaction>) -> Vec<BatcherTransactionData> {
        block
            .transactions
            .iter()
            .filter(|tx| {
                tx.from == self.config.chain.batch_sender
                    && tx
                        .to
                        .map(|to| to == self.config.chain.batch_inbox)
                        .unwrap_or(false)
            })
            .map(|tx| tx.input.to_vec())
            .collect()
    }

    async fn get_deposits(&self, block_num: u64, block_hash: H256) -> Result<Vec<UserDeposited>> {
        let deposit_event = "TransactionDeposited(address,address,uint256,bytes)";
        let deposit_topic = H256::from_slice(&keccak256(deposit_event));

        let deposit_filter = Filter::new()
            .address(self.config.chain.deposit_contract)
            .topic0(deposit_topic)
            .from_block(block_num)
            .to_block(block_num);

        let deposit_logs = self.provider.get_logs(&deposit_filter).await?;

        Ok(deposit_logs
            .into_iter()
            .map(|log| UserDeposited::from_log(log, block_num, block_hash).unwrap())
            .collect())
    }

    fn channels_full(&self) -> bool {
        self.tx_sender.capacity() == 0
            || self.block_sender.capacity() == 0
            || self.deposit_sender.capacity() == 0
    }
}

struct Receivers {
    tx_receiver: Receiver<BatcherTransactionData>,
    block_receiver: Receiver<Block<Transaction>>,
    deposit_receiver: Receiver<UserDeposited>,
}

fn start_watcher(start_block: u64, config: Arc<Config>) -> Result<(JoinHandle<()>, Receivers)> {
    let (tx_sender, tx_receiver) = channel(1000);
    let (block_sender, block_receiver) = channel(1000);
    let (deposit_sender, deposit_receiver) = channel(1000);

    let watcher = InnerWatcher::new(config, tx_sender, block_sender, deposit_sender)?;

    let handle = spawn(async move {
        let mut block_num = start_block;

        loop {
            tracing::debug!("fetching L1 data for block {}", block_num);
            if watcher.try_ingest_block(block_num).await.is_ok() {
                block_num += 1;
            } else {
                tracing::warn!("failed to fetch data for block {}", block_num);
            }
        }
    });

    let receivers = Receivers {
        tx_receiver,
        block_receiver,
        deposit_receiver,
    };

    Ok((handle, receivers))
}

type BatcherTransactionData = Vec<u8>;
