use std::{
    cmp::max,
    collections::HashMap,
    sync::Arc,
    time::{Duration, SystemTime},
};

use ethers::{
    providers::{Http, HttpRateLimitRetryPolicy, Middleware, Provider, RetryClient},
    types::Log,
};
use ethers::{
    types::{Address, Block, BlockNumber, Filter, Transaction, H256, U256},
    utils::keccak256,
};

use eyre::Result;
use once_cell::sync::Lazy;
use reqwest::Url;
use tokio::{spawn, sync::mpsc, task::JoinHandle, time::sleep};

use crate::{
    common::BlockInfo,
    config::{Config, SystemConfig},
    derive::stages::attributes::UserDeposited,
};

pub mod utils;

static CONFIG_UPDATE_TOPIC: Lazy<H256> =
    Lazy::new(|| H256::from_slice(&keccak256("ConfigUpdate(uint256,uint8,bytes)")));

static TRANSACTION_DEPOSITED_TOPIC: Lazy<H256> = Lazy::new(|| {
    H256::from_slice(&keccak256(
        "TransactionDeposited(address,address,uint256,bytes)",
    ))
});

/// Handles watching the L1 chain and monitoring for new blocks, deposits,
/// and batcher transactions. The monitoring loop is spawned in a seperate
/// task and communication happens via the internal channels. When ChainWatcher
/// is dropped, the monitoring task is automatically aborted.
pub struct ChainWatcher {
    /// Task handle for the monitoring loop
    handle: Option<JoinHandle<()>>,
    /// Global config
    config: Arc<Config>,
    /// The L1 starting block
    l1_start_block: u64,
    /// The L2 starting block
    l2_start_block: u64,
    /// Channel for receiving block updates for each new block
    block_update_receiver: Option<mpsc::Receiver<BlockUpdate>>,
}

/// Updates L1Info
pub enum BlockUpdate {
    /// A new block extending the current chain
    NewBlock(Box<L1Info>),
    /// Updates the most recent finalized block
    FinalityUpdate(u64),
    /// Reorg detected
    Reorg,
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
    /// L1 state root
    pub state_root: H256,
}

/// Watcher actually ingests the L1 blocks. Should be run in another
/// thread and called periodically to keep updating channels
struct InnerWatcher {
    /// Global Config
    config: Arc<Config>,
    /// Ethers provider for L1
    provider: Arc<Provider<RetryClient<Http>>>,
    /// Channel to send block updates
    block_update_sender: mpsc::Sender<BlockUpdate>,
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
    /// Current system config value
    system_config: SystemConfig,
    /// Next system config if it exists and the L1 block number it activates
    system_config_update: (u64, Option<SystemConfig>),
}

type BatcherTransactionData = Vec<u8>;

impl Drop for ChainWatcher {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

impl ChainWatcher {
    /// Creates a new ChainWatcher and begins the monitoring task.
    /// Errors if the rpc url in the config is invalid.
    pub fn new(l1_start_block: u64, l2_start_block: u64, config: Arc<Config>) -> Result<Self> {
        Ok(Self {
            handle: None,
            config,
            l1_start_block,
            l2_start_block,
            block_update_receiver: None,
        })
    }

    /// Starts the chain watcher at the given block numbers
    pub fn start(&mut self) -> Result<()> {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }

        let (handle, recv) = start_watcher(
            self.l1_start_block,
            self.l2_start_block,
            self.config.clone(),
        )?;

        self.handle = Some(handle);
        self.block_update_receiver = Some(recv);

        Ok(())
    }

    /// Resets the chain watcher at the given block numbers
    pub fn restart(&mut self, l1_start_block: u64, l2_start_block: u64) -> Result<()> {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }

        let (handle, recv) = start_watcher(l1_start_block, l2_start_block, self.config.clone())?;

        self.handle = Some(handle);
        self.block_update_receiver = Some(recv);
        self.l1_start_block = l1_start_block;
        self.l2_start_block = l2_start_block;

        Ok(())
    }

    /// Attempts to receive a message from the block update channel.
    /// Returns an error if the channel contains no messages.
    pub fn try_recv_from_channel(&mut self) -> Result<BlockUpdate> {
        let receiver = self
            .block_update_receiver
            .as_mut()
            .ok_or(eyre::eyre!("the watcher hasn't started"))?;

        receiver.try_recv().map_err(eyre::Report::from)
    }

    /// Asynchronously receives from the block update channel.
    /// Returns `None` if the channel contains no messages.
    pub async fn recv_from_channel(&mut self) -> Option<BlockUpdate> {
        match &mut self.block_update_receiver {
            Some(receiver) => receiver.recv().await,
            None => None,
        }
    }
}

impl InnerWatcher {
    async fn new(
        config: Arc<Config>,
        block_update_sender: mpsc::Sender<BlockUpdate>,
        l1_start_block: u64,
        l2_start_block: u64,
    ) -> Self {
        let provider = generate_http_provider(&config.l1_rpc_url);

        let system_config = if l2_start_block == config.chain.l2_genesis.number
            || !config.chain.meta.enable_config_updates
        {
            config.chain.system_config
        } else {
            let l2_provider = generate_http_provider(&config.l2_rpc_url);

            let block = l2_provider
                .get_block_with_txs(l2_start_block - 1)
                .await
                .unwrap()
                .unwrap();

            let input = &block
                .transactions
                .first()
                .expect(
                    "Could not find the L1 attributes deposited transaction in the parent L2 block",
                )
                .input;

            let batch_sender = Address::from_slice(&input[176..196]);
            let l1_fee_overhead = U256::from(H256::from_slice(&input[196..228]).as_bytes());
            let l1_fee_scalar = U256::from(H256::from_slice(&input[228..260]).as_bytes());

            SystemConfig {
                batch_sender,
                l1_fee_overhead,
                l1_fee_scalar,
                gas_limit: block.gas_limit,
                // TODO: fetch from contract
                unsafe_block_signer: config.chain.system_config.unsafe_block_signer,
            }
        };

        Self {
            config,
            provider,
            block_update_sender,
            current_block: l1_start_block,
            head_block: 0,
            finalized_block: 0,
            unfinalized_blocks: Vec::new(),
            deposits: HashMap::new(),
            system_config,
            system_config_update: (l1_start_block, None),
        }
    }

    async fn try_ingest_block(&mut self) -> Result<()> {
        let now = SystemTime::now();
        if self.current_block > self.finalized_block {
            let finalized_block = self.get_finalized().await?;

            // Only update finalized block if it has changed to avoid spamming the channel.
            if self.finalized_block < finalized_block {
                tracing::debug!("[l1] finalized block updated to {}", finalized_block);
                self.finalized_block = finalized_block;
                self.block_update_sender
                    .send(BlockUpdate::FinalityUpdate(finalized_block))
                    .await?;
                self.unfinalized_blocks
                    .retain(|b| b.number > self.finalized_block)
            }
        }

        if self.current_block > self.head_block {
            let head_block = self.get_head().await?;
            self.head_block = head_block;
        }

        if self.current_block <= self.head_block {
            if self.config.chain.meta.enable_config_updates {
                self.update_system_config().await?;
            }

            let block = self.get_block(self.current_block).await?;
            let user_deposits = if self.config.chain.meta.enable_deposited_txs {
                self.get_deposits(self.current_block).await?
            } else {
                Vec::new()
            };
            let finalized = self.current_block >= self.finalized_block;

            let l1_info = L1Info::new(
                &block,
                user_deposits,
                self.config.chain.batch_inbox,
                finalized,
                self.system_config,
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

            self.block_update_sender.send(update).await?;

            self.current_block += 1;
        } else {
            let watcher_delay = self.config.watcher_delay;
            let elapsed = now.elapsed().unwrap_or_default().as_millis() as u64;
            let delay = max(0, watcher_delay.saturating_sub(elapsed));
            sleep(Duration::from_millis(delay)).await;
        }

        Ok(())
    }

    async fn update_system_config(&mut self) -> Result<()> {
        let (last_update_block, _) = self.system_config_update;

        if last_update_block < self.current_block {
            let to_block = last_update_block + 1000;
            let filter = Filter::new()
                .address(self.config.chain.system_config_contract)
                .topic0(*CONFIG_UPDATE_TOPIC)
                .from_block(last_update_block + 1)
                .to_block(to_block);

            let updates = self.provider.get_logs(&filter).await?;
            let update = updates.into_iter().next();

            let update_block = update.as_ref().and_then(|update| update.block_number);
            let update = update.and_then(|update| SystemConfigUpdate::try_from(update).ok());

            if let Some((update_block, update)) = update_block.zip(update) {
                let mut config = self.system_config;
                match update {
                    SystemConfigUpdate::BatchSender(addr) => {
                        config.batch_sender = addr;
                    }
                    SystemConfigUpdate::Fees(overhead, scalar) => {
                        config.l1_fee_overhead = overhead;
                        config.l1_fee_scalar = scalar;
                    }
                    SystemConfigUpdate::Gas(gas) => {
                        config.gas_limit = gas;
                    }
                    SystemConfigUpdate::UnsafeBlockSigner(addr) => {
                        config.unsafe_block_signer = addr;
                    }
                }

                self.system_config_update = (update_block.as_u64(), Some(config));
            } else {
                self.system_config_update = (to_block, None);
            }
        }

        let (last_update_block, next_config) = self.system_config_update;

        if last_update_block == self.current_block {
            if let Some(next_config) = next_config {
                tracing::info!("system config updated");
                tracing::debug!("{:?}", next_config);
                self.system_config = next_config;
            }
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
        let block_number = match self.config.devnet {
            false => BlockNumber::Finalized,
            true => BlockNumber::Latest,
        };

        Ok(self
            .provider
            .get_block(block_number)
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
                let end_block = self.head_block.min(block_num + 1000);

                let deposit_filter = Filter::new()
                    .address(self.config.chain.deposit_contract)
                    .topic0(*TRANSACTION_DEPOSITED_TOPIC)
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
        batch_inbox: Address,
        finalized: bool,
        system_config: SystemConfig,
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
            state_root: block.state_root,
        };

        let batcher_transactions =
            create_batcher_transactions(block, system_config.batch_sender, batch_inbox);

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
    l1_start_block: u64,
    l2_start_block: u64,
    config: Arc<Config>,
) -> Result<(JoinHandle<()>, mpsc::Receiver<BlockUpdate>)> {
    let (block_update_sender, block_update_receiver) = mpsc::channel(1000);

    let handle = spawn(async move {
        let mut watcher =
            InnerWatcher::new(config, block_update_sender, l1_start_block, l2_start_block).await;

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

enum SystemConfigUpdate {
    BatchSender(Address),
    Fees(U256, U256),
    Gas(U256),
    UnsafeBlockSigner(Address),
}

impl TryFrom<Log> for SystemConfigUpdate {
    type Error = eyre::Report;

    fn try_from(log: Log) -> Result<Self> {
        let version = log
            .topics
            .get(1)
            .ok_or(eyre::eyre!("invalid system config update"))?
            .to_low_u64_be();

        if version != 0 {
            return Err(eyre::eyre!("invalid system config update"));
        }

        let update_type = log
            .topics
            .get(2)
            .ok_or(eyre::eyre!("invalid system config update"))?
            .to_low_u64_be();

        match update_type {
            0 => {
                let addr_bytes = log
                    .data
                    .get(76..96)
                    .ok_or(eyre::eyre!("invalid system config update"))?;

                let addr = Address::from_slice(addr_bytes);
                Ok(Self::BatchSender(addr))
            }
            1 => {
                let fee_overhead = log
                    .data
                    .get(64..96)
                    .ok_or(eyre::eyre!("invalid system config update"))?;

                let fee_scalar = log
                    .data
                    .get(96..128)
                    .ok_or(eyre::eyre!("invalid system config update"))?;

                let fee_overhead = U256::from_big_endian(fee_overhead);
                let fee_scalar = U256::from_big_endian(fee_scalar);

                Ok(Self::Fees(fee_overhead, fee_scalar))
            }
            2 => {
                let gas_bytes = log
                    .data
                    .get(64..96)
                    .ok_or(eyre::eyre!("invalid system config update"))?;

                let gas = U256::from_big_endian(gas_bytes);
                Ok(Self::Gas(gas))
            }
            3 => {
                let addr_bytes = log
                    .data
                    .get(76..96)
                    .ok_or(eyre::eyre!("invalid system config update"))?;

                let addr = Address::from_slice(addr_bytes);
                Ok(Self::UnsafeBlockSigner(addr))
            }
            _ => Err(eyre::eyre!("invalid system config update")),
        }
    }
}

fn generate_http_provider(url: &str) -> Arc<Provider<RetryClient<Http>>> {
    let client = reqwest::ClientBuilder::new()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let http = Http::new_with_client(Url::parse(url).expect("ivnalid rpc url"), client);
    let policy = Box::new(HttpRateLimitRetryPolicy);
    let client = RetryClient::new(http, policy, 100, 50);
    Arc::new(Provider::new(client))
}
