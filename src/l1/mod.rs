use std::{iter, str::FromStr, sync::Arc, time::Duration};

use ethers_core::{
    abi::Address,
    types::{Block, BlockNumber, Filter, Transaction, H256, U256},
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
    pub block_info: L1BlockInfo,
    /// The system config at the block
    pub system_config: SystemConfig,
    /// User deposits from that block
    pub user_deposits: Vec<UserDeposited>,
}

/// L1 block info
#[derive(Debug)]
pub struct L1BlockInfo {
    /// L1 block number
    pub number: u64,
    /// L1 block hash
    pub hash: H256,
    /// L1 block timestamp
    pub timestamp: u64,
    /// L1 base fee per gas
    pub base_fee: U256,
    /// L1 mix hash (prevrandao)
    pub mix_hash: H256,
}

#[derive(Debug)]
/// Optimism system config contract values
pub struct SystemConfig {
    /// Batch sender address
    pub batch_sender: Address,
    /// L2 gas limit
    pub gas_limit: U256,
    /// Fee overhead
    pub l1_fee_overhead: U256,
    /// Fee scalar
    pub l1_fee_scalar: U256,
}

/// Watcher actually ingests the L1 blocks. Should be run in another
/// thread and called periodically to keep updating channels
struct InnerWatcher {
    /// Global Config
    config: Arc<Config>,
    /// Ethers provider for L1
    provider: Provider<RetryClient<Http>>,
    /// Channel to send batch tx data
    tx_sender: Sender<BatcherTransactionData>,
    /// Channel to send L1Info
    l1_info_sender: Sender<L1Info>,
    /// Most recent ingested block
    current_block: u64,
    /// Most recent finalized block
    finalized_block: u64,
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
        start_block: u64,
    ) -> Result<Self> {
        let http =
            Http::from_str(&config.l1_rpc_url).map_err(|_| eyre::eyre!("invalid L1 RPC URL"))?;
        let policy = Box::new(HttpRateLimitRetryPolicy);
        let client = RetryClient::new(http, policy, 100, 50);
        let provider = Provider::new(client);

        Ok(Self {
            config,
            provider,
            tx_sender,
            l1_info_sender,
            current_block: start_block,
            finalized_block: 0,
        })
    }

    async fn try_ingest_block(&mut self) -> Result<()> {
        let finalized_block = self.get_finalized().await?;
        self.finalized_block = finalized_block;

        if !self.channels_full() && self.current_block <= self.finalized_block {
            let block = self.get_block(self.current_block).await?;
            let l1_info = self.get_l1_info(&block).await?;
            let batcher_transactions = self.get_batcher_transactions(&block);

            self.l1_info_sender.send(l1_info).await.unwrap();

            for tx in batcher_transactions {
                self.tx_sender.send(tx).await?;
            }

            self.current_block += 1;
        } else {
            tracing::warn!("l1 watcher sleeping");
            tracing::warn!("chanel full: {}", self.channels_full());
            sleep(Duration::from_millis(250)).await;
        }

        Ok(())
    }

    async fn get_finalized(&self) -> Result<u64> {
        Ok(self
            .provider
            .get_block(BlockNumber::Finalized)
            .await?
            .ok_or(eyre::eyre!("block not found"))?
            .number
            .ok_or(eyre::eyre!("block pending"))?
            .as_u64())
    }

    async fn get_block(&self, block_num: u64) -> Result<Block<Transaction>> {
        self.provider
            .get_block_with_txs(block_num)
            .await?
            .ok_or(eyre::eyre!("block not found"))
    }

    async fn get_l1_info(&self, block: &Block<Transaction>) -> Result<L1Info> {
        let block_number = block
            .number
            .ok_or(eyre::eyre!("block not included"))?
            .as_u64();

        let block_hash = block.hash.ok_or(eyre::eyre!("block not included"))?;

        let block_info = L1BlockInfo {
            number: block_number,
            hash: block_hash,
            timestamp: block.timestamp.as_u64(),
            base_fee: block
                .base_fee_per_gas
                .ok_or(eyre::eyre!("block is pre london"))?,
            mix_hash: block.mix_hash.ok_or(eyre::eyre!("block not included"))?,
        };

        let system_config = SystemConfig {
            batch_sender: self.config.chain.batch_sender,
            gas_limit: U256::from(25_000_000),
            l1_fee_overhead: U256::from(2100),
            l1_fee_scalar: U256::from(1000000),
        };

        let user_deposits = self.get_deposits(block_number, block_hash).await?;

        Ok(L1Info {
            block_info,
            system_config,
            user_deposits,
        })
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

    let mut watcher = InnerWatcher::new(config, tx_sender, l1_info_sender, start_block)?;

    let handle = spawn(async move {
        loop {
            tracing::debug!("fetching L1 data for block {}", watcher.current_block);
            let start = std::time::SystemTime::now();
            if watcher.try_ingest_block().await.is_err() {
                tracing::warn!("failed to fetch data for block {}", watcher.current_block);
            }
            let end = std::time::SystemTime::now();
            let duration = end.duration_since(start).unwrap().as_millis();
            tracing::info!(target: "magi", "ingest time ms: {}", duration);
        }
    });

    let receivers = Receivers {
        tx_receiver,
        l1_info_receiver,
    };
    Ok((handle, receivers))
}

impl SystemConfig {
    /// Encoded batch sender as a H256
    pub fn batcher_hash(&self) -> H256 {
        let mut batch_sender_bytes = self.batch_sender.as_bytes().to_vec();
        let mut batcher_hash = iter::repeat(0).take(12).collect::<Vec<_>>();
        batcher_hash.append(&mut batch_sender_bytes);
        H256::from_slice(&batcher_hash)
    }
}
