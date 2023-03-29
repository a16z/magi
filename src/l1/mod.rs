use std::{
    collections::HashMap,
    iter,
    str::FromStr,
    sync::{
        mpsc::{sync_channel, Receiver, SyncSender},
        Arc,
    },
    time::Duration,
};

use ethers_core::{
    abi::Address,
    types::{Block, BlockNumber, Filter, Transaction, H256, U256},
    utils::keccak256,
};
use ethers_providers::{Http, HttpRateLimitRetryPolicy, Middleware, Provider, RetryClient};

use eyre::Result;
use tokio::{spawn, task::JoinHandle, time::sleep};

use crate::{common::BlockInfo, config::Config, derive::stages::attributes::UserDeposited};

/// Handles watching the L1 chain and monitoring for new blocks, deposits,
/// and batcher transactions. The monitoring loop is spawned in a seperate
/// task and communication happens via the internal channels. When ChainWatcher
/// is dropped, the monitoring task is automatically aborted.
pub struct ChainWatcher {
    /// Task handle for the monitoring loop
    handle: JoinHandle<()>,
    /// Global config
    config: Arc<Config>,
    /// Channel for receiving block updates for each new block
    pub block_update_receiver: Receiver<BlockUpdate>,
}

/// Updates L1Info
pub enum BlockUpdate {
    /// A new block extending the current chain
    NewBlock(Box<L1Info>),
    /// Updates the most recent finalized block
    FinalityUpdate(u64),
    /// Reorg detected
    Reorg,
    /// Reached the chain head
    HeadReached,
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
    /// Batcher transactions in block
    pub batcher_transactions: Vec<BatcherTransactionData>,
    /// Whether the block has finalized
    pub finalized: bool,
}

/// L1 block info
#[derive(Debug, Clone)]
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
    /// Channel to send block updates
    block_update_sender: SyncSender<BlockUpdate>,
    /// Most recent ingested block
    current_block: u64,
    /// Most recent block
    head_block: u64,
    /// Most recent finalized block
    finalized_block: u64,
    /// List of blocks that have not been finalized yet
    unfinalized_blocks: Vec<BlockInfo>,
    /// Mapping from block number to user deposits. Past block deposits
    /// are removed as they are no longer needed
    deposits: HashMap<u64, Vec<UserDeposited>>,
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
        let (handle, block_updates) = start_watcher(start_block, config.clone())?;

        Ok(Self {
            handle,
            config,
            block_update_receiver: block_updates,
        })
    }

    pub fn reset(&mut self, start_block: u64) -> Result<()> {
        self.handle.abort();

        let (handle, recv) = start_watcher(start_block, self.config.clone())?;

        self.handle = handle;
        self.block_update_receiver = recv;

        Ok(())
    }
}

impl InnerWatcher {
    fn new(
        config: Arc<Config>,
        block_update_sender: SyncSender<BlockUpdate>,
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
            block_update_sender,
            current_block: start_block,
            head_block: 0,
            finalized_block: 0,
            unfinalized_blocks: Vec::new(),
            deposits: HashMap::new(),
        })
    }

    async fn try_ingest_block(&mut self) -> Result<()> {
        if self.current_block > self.finalized_block {
            let finalized_block = self.get_finalized().await?;

            self.finalized_block = finalized_block;
            self.block_update_sender
                .send(BlockUpdate::FinalityUpdate(finalized_block))?;

            self.unfinalized_blocks
                .retain(|b| b.number > self.finalized_block)
        }

        if self.current_block > self.head_block {
            let head_block = self.get_head().await?;
            self.head_block = head_block;
        }

        if self.current_block <= self.head_block {
            let block = self.get_block(self.current_block).await?;
            let user_deposits = self.get_deposits(self.current_block).await?;
            let finalized = self.current_block >= self.finalized_block;

            let l1_info = L1Info::new(
                &block,
                user_deposits,
                self.config.chain.batch_sender,
                self.config.chain.batch_inbox,
                finalized,
            )?;

            if l1_info.block_info.number >= self.finalized_block {
                let block_info = BlockInfo {
                    hash: l1_info.block_info.hash,
                    number: l1_info.block_info.number,
                    timestamp: l1_info.block_info.timestamp,
                    parent_hash: block.parent_hash,
                };

                self.unfinalized_blocks.push(block_info);
            }

            let update = if self.check_reorg() {
                BlockUpdate::Reorg
            } else {
                BlockUpdate::NewBlock(Box::new(l1_info))
            };

            self.block_update_sender.send(update)?;

            self.current_block += 1;
        } else {
            tracing::debug!("L1 head reached");
            self.block_update_sender.send(BlockUpdate::HeadReached)?;
            sleep(Duration::from_millis(250)).await;
        }

        Ok(())
    }

    fn check_reorg(&self) -> bool {
        let len = self.unfinalized_blocks.len();
        if len >= 2 {
            let last = self.unfinalized_blocks[len - 1];
            let parent = self.unfinalized_blocks[len - 2];
            last.parent_hash != parent.hash
        } else {
            false
        }
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

    async fn get_head(&self) -> Result<u64> {
        Ok(self
            .provider
            .get_block(BlockNumber::Latest)
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

    async fn get_deposits(&mut self, block_num: u64) -> Result<Vec<UserDeposited>> {
        match self.deposits.remove(&block_num) {
            Some(deposits) => Ok(deposits),
            None => {
                let deposit_event = "TransactionDeposited(address,address,uint256,bytes)";
                let deposit_topic = H256::from_slice(&keccak256(deposit_event));

                let end_block = self.head_block.min(block_num + 1000);

                let deposit_filter = Filter::new()
                    .address(self.config.chain.deposit_contract)
                    .topic0(deposit_topic)
                    .from_block(block_num)
                    .to_block(end_block);

                let deposit_logs = self
                    .provider
                    .get_logs(&deposit_filter)
                    .await?
                    .into_iter()
                    .map(|log| UserDeposited::try_from(log).unwrap())
                    .collect::<Vec<UserDeposited>>();

                for num in block_num..=end_block {
                    let deposits = deposit_logs
                        .iter()
                        .filter(|d| d.l1_block_num == num)
                        .cloned()
                        .collect();

                    self.deposits.insert(num, deposits);
                }

                Ok(self.deposits.remove(&block_num).unwrap())
            }
        }
    }
}

impl L1Info {
    pub fn new(
        block: &Block<Transaction>,
        user_deposits: Vec<UserDeposited>,
        batch_sender: Address,
        batch_inbox: Address,
        finalized: bool,
    ) -> Result<Self> {
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
            batch_sender,
            gas_limit: U256::from(25_000_000),
            l1_fee_overhead: U256::from(2100),
            l1_fee_scalar: U256::from(1000000),
        };

        let batcher_transactions = create_batcher_transactions(block, batch_sender, batch_inbox);

        Ok(L1Info {
            block_info,
            system_config,
            user_deposits,
            batcher_transactions,
            finalized,
        })
    }
}

fn create_batcher_transactions(
    block: &Block<Transaction>,
    batch_sender: Address,
    batch_inbox: Address,
) -> Vec<BatcherTransactionData> {
    block
        .transactions
        .iter()
        .filter(|tx| tx.from == batch_sender && tx.to.map(|to| to == batch_inbox).unwrap_or(false))
        .map(|tx| tx.input.to_vec())
        .collect()
}

fn start_watcher(
    start_block: u64,
    config: Arc<Config>,
) -> Result<(JoinHandle<()>, Receiver<BlockUpdate>)> {
    let (block_update_sender, block_update_receiver) = sync_channel(1000);

    let mut watcher = InnerWatcher::new(config, block_update_sender, start_block)?;

    let handle = spawn(async move {
        loop {
            tracing::debug!("fetching L1 data for block {}", watcher.current_block);
            if let Err(err) = watcher.try_ingest_block().await {
                tracing::warn!(
                    "failed to fetch data for block {}: {}",
                    watcher.current_block,
                    err
                );
            }
        }
    });

    Ok((handle, block_update_receiver))
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
