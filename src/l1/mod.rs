use std::{str::FromStr, sync::Arc, time::Duration};

use ethers_core::{
    types::{Block, Filter, Transaction, H256, U256},
    utils::keccak256, abi::Address,
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

/// Handles watching the L1 chain and monitoring for new blocks, deposits,
/// and batcher transactions. The monitoring loop is spawned in a seperate
/// task and communication happens via the internal channels. When ChainWatcher
/// is dropped, the monitoring task is automatically aborted.
pub struct ChainWatcher {
    /// Task handle for the monitoring loop
    handle: JoinHandle<()>,
    /// Channel for receiving batcher transactions
    pub tx_receiver: Option<Receiver<BatcherTransactionData>>,
    /// Channel for receiving L1Info for each new block
    pub l1_info_receiver: Receiver<L1Info>,
}

/// Data tied to a specific L1 block
#[derive(Debug)]
pub struct L1Info {
    /// L1 block data
    pub block_info: BlockInfo,
    /// The system config at the block
    pub system_config: SystemConfig,
    /// User deposits from that block
    pub user_deposits: Vec<UserDeposited>,

}

#[derive(Debug)]
pub struct BlockInfo {
    pub number: u64,
    pub hash: H256,
    pub timestamp: u64,
    pub base_fee: U256,
    pub mix_hash: H256,
}

#[derive(Debug)]
pub struct SystemConfig {
    pub batch_sender: Address,
    pub gas_limit: U256,
    pub l1_fee_overhead: U256,
    pub l1_fee_scalar: U256,
}

struct InnerWatcher {
    config: Arc<Config>,
    provider: Provider<RetryClient<Http>>,
    tx_sender: Sender<BatcherTransactionData>,
    l1_info_sender: Sender<L1Info>,
}

struct Receivers {
    tx_receiver: Receiver<BatcherTransactionData>,
    l1_info_receiver: Receiver<L1Info>,
}

type BatcherTransactionData = Vec<u8>;

impl Drop for ChainWatcher {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

impl ChainWatcher {
    /// Creates a new ChainWatcher and begins the monitoring task.
    /// Errors if the rpc url in the config is invalid.
    pub fn new(start_block: u64, config: Arc<Config>) -> Result<Self> {
        let (handle, receivers) = start_watcher(start_block, config)?;

        Ok(Self {
            handle,
            tx_receiver: Some(receivers.tx_receiver),
            l1_info_receiver: receivers.l1_info_receiver,
        })
    }

    /// Takes ownership of the batcher transaction receiver. Returns None
    /// if the receiver has already been taken.
    pub fn take_tx_receiver(&mut self) -> Option<Receiver<BatcherTransactionData>> {
        self.tx_receiver.take()
    }
}

impl InnerWatcher {
    fn new(
        config: Arc<Config>,
        tx_sender: Sender<BatcherTransactionData>,
        l1_info_sender: Sender<L1Info>,
    ) -> Result<Self> {
        let http = Http::from_str(&config.l1_rpc).map_err(|_| eyre::eyre!("invalid L1 RPC URL"))?;
        let policy = Box::new(HttpRateLimitRetryPolicy);
        let client = RetryClient::new(http, policy, 100, 50);
        let provider = Provider::new(client);

        Ok(Self {
            config,
            provider,
            tx_sender,
            l1_info_sender,
        })
    }

    async fn try_ingest_block(&self, block_num: u64) -> Result<()> {
        if !self.channels_full() {
            let block = self.get_block(block_num).await?;
            let l1_info = self.get_l1_info(&block).await?;
            let batcher_transactions = self.get_batcher_transactions(&block);

            self.l1_info_sender.send(l1_info).await.unwrap();

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

    async fn get_l1_info(&self, block: &Block<Transaction>) -> Result<L1Info> {
        let block_number = block.number.ok_or(eyre::eyre!("block not included"))?.as_u64();
        let block_hash = block.hash.ok_or(eyre::eyre!("block not included"))?;

        let block_info = BlockInfo {
            number: block_number,
            hash: block_hash,
            timestamp: block.timestamp.as_u64(),
            base_fee: block.base_fee_per_gas.ok_or(eyre::eyre!("block is pre london"))?,
            mix_hash: block.mix_hash.ok_or(eyre::eyre!("block not included"))?,
        };

        let system_config = SystemConfig {
            batch_sender: self.config.chain.batch_sender,
            gas_limit: U256::from(30_000_000),
            l1_fee_overhead: U256::from(2100),
            l1_fee_scalar: U256::from(1000000),
        };

        let user_deposits = self.get_deposits(block_number, block_hash).await?;

        Ok(L1Info { block_info, system_config, user_deposits })
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
        self.tx_sender.capacity() == 0 || self.l1_info_sender.capacity() == 0
    }
}

fn start_watcher(start_block: u64, config: Arc<Config>) -> Result<(JoinHandle<()>, Receivers)> {
    let (tx_sender, tx_receiver) = channel(1000);
    let (l1_info_sender, l1_info_receiver) = channel(1000);

    let watcher = InnerWatcher::new(config, tx_sender, l1_info_sender)?;

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

    let receivers = Receivers { tx_receiver, l1_info_receiver };
    Ok((handle, receivers))
}

