use std::{collections::HashMap, sync::Arc, time::Duration};

use bytes::Bytes;
use ethers::{
    providers::{Http, HttpRateLimitRetryPolicy, Middleware, Provider, RetryClient},
    types::{Address, Block, BlockNumber, Filter, Transaction, H256},
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
    l1::decode_blob_data,
};

use super::{l1_info::L1BlockInfo, BlobFetcher, L1Info, SystemConfigUpdate};

static CONFIG_UPDATE_TOPIC: Lazy<H256> =
    Lazy::new(|| H256::from_slice(&keccak256("ConfigUpdate(uint256,uint8,bytes)")));

static TRANSACTION_DEPOSITED_TOPIC: Lazy<H256> = Lazy::new(|| {
    H256::from_slice(&keccak256(
        "TransactionDeposited(address,address,uint256,bytes)",
    ))
});

/// The transaction type used to identify transactions that carry blobs
/// according to EIP 4844.
const BLOB_CARRYING_TRANSACTION_TYPE: u64 = 3;

/// The data contained in a batcher transaction.
/// The actual source of this data can be either calldata or blobs.
pub type BatcherTransactionData = Bytes;

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

/// Watcher actually ingests the L1 blocks. Should be run in another
/// thread and called periodically to keep updating channels
struct InnerWatcher {
    /// Global Config
    config: Arc<Config>,
    /// Ethers provider for L1
    provider: Arc<Provider<RetryClient<Http>>>,
    /// L1 beacon node to fetch blobs
    blob_fetcher: Arc<BlobFetcher>,
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
        let blob_fetcher = Arc::new(BlobFetcher::new(config.l1_beacon_url.clone()));

        let system_config = if l2_start_block == config.chain.l2_genesis.number {
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
            let l1_fee_overhead = alloy_primitives::U256::from_be_slice(&input[196..228]);
            let l1_fee_scalar = alloy_primitives::U256::from_be_slice(&input[228..260]);
            let mut gas_limit: [u8; 32] = [0; 32];
            block.gas_limit.to_big_endian(&mut gas_limit);
            let gas_limit = alloy_primitives::U256::from_be_slice(&gas_limit);

            SystemConfig {
                batch_sender: alloy_primitives::Address::from_slice(batch_sender.as_bytes()),
                l1_fee_overhead,
                l1_fee_scalar,
                gas_limit,
                // TODO: fetch from contract
                unsafe_block_signer: config.chain.system_config.unsafe_block_signer,
            }
        };

        Self {
            config,
            provider,
            blob_fetcher,
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
            self.update_system_config().await?;

            let block = self.get_block(self.current_block).await?;
            let user_deposits = self.get_deposits(self.current_block).await?;
            let batcher_transactions = self.get_batcher_transactions(&block).await?;

            let finalized = self.current_block >= self.finalized_block;

            let l1_info = L1Info {
                system_config: self.system_config,
                block_info: L1BlockInfo::try_from(&block)?,
                batcher_transactions,
                user_deposits,
                finalized,
            };

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
            sleep(Duration::from_millis(250)).await;
        }

        Ok(())
    }

    async fn update_system_config(&mut self) -> Result<()> {
        let (last_update_block, _) = self.system_config_update;

        if last_update_block < self.current_block {
            let to_block = last_update_block + 1000;
            let filter = Filter::new()
                .address(ethers::types::Address::from_slice(
                    self.config.chain.system_config_contract.as_slice(),
                ))
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
                        config.batch_sender =
                            alloy_primitives::Address::from_slice(addr.as_bytes());
                    }
                    SystemConfigUpdate::Fees(overhead, scalar) => {
                        let mut oh: [u8; 32] = [0; 32];
                        overhead.to_big_endian(&mut oh);
                        config.l1_fee_overhead = alloy_primitives::U256::from_be_bytes(oh);
                        let mut s: [u8; 32] = [0; 32];
                        scalar.to_big_endian(&mut s);
                        config.l1_fee_scalar = alloy_primitives::U256::from_be_bytes(s);
                    }
                    SystemConfigUpdate::Gas(gas) => {
                        let mut g: [u8; 32] = [0; 32];
                        gas.to_big_endian(&mut g);
                        config.gas_limit = alloy_primitives::U256::from_be_bytes(g);
                    }
                    SystemConfigUpdate::UnsafeBlockSigner(addr) => {
                        config.unsafe_block_signer =
                            alloy_primitives::Address::from_slice(addr.as_bytes());
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
                    .address(ethers::types::Address::from_slice(
                        self.config.chain.deposit_contract.as_slice(),
                    ))
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

    /// Given a block, return a list of [`BatcherTransactionData`] containing either the
    /// calldata or the decoded blob data for each batcher transaction in the block.
    pub async fn get_batcher_transactions(
        &self,
        block: &Block<Transaction>,
    ) -> Result<Vec<BatcherTransactionData>> {
        let mut batcher_transactions_data = Vec::new();
        let mut indexed_blobs = Vec::new();
        let mut blob_index = 0;

        for tx in block.transactions.iter() {
            let tx_blob_hashes: Vec<String> = tx
                .other
                .get_deserialized("blobVersionedHashes")
                .unwrap_or(Ok(Vec::new()))
                .unwrap_or_default();

            if !self.is_valid_batcher_transaction(tx) {
                blob_index += tx_blob_hashes.len();
                continue;
            }

            let tx_type = tx.transaction_type.map(|t| t.as_u64()).unwrap_or(0);
            if tx_type != BLOB_CARRYING_TRANSACTION_TYPE {
                batcher_transactions_data.push(tx.input.0.clone());
                continue;
            }

            for blob_hash in tx_blob_hashes {
                indexed_blobs.push((blob_index, blob_hash));
                blob_index += 1;
            }
        }

        // if at this point there are no blobs, return early
        if indexed_blobs.is_empty() {
            return Ok(batcher_transactions_data);
        }

        let slot = self
            .blob_fetcher
            .get_slot_from_time(block.timestamp.as_u64())
            .await?;

        // perf: fetch only the required indexes instead of all
        let blobs = self.blob_fetcher.fetch_blob_sidecars(slot).await?;
        tracing::debug!("fetched {} blobs for slot {}", blobs.len(), slot);

        for (blob_index, _) in indexed_blobs {
            let Some(blob_sidecar) = blobs.iter().find(|b| b.index == blob_index as u64) else {
                // This can happen in the case the blob retention window has expired
                // and the data is no longer available. This case is not handled yet.
                eyre::bail!("blob index {} not found in fetched sidecars", blob_index);
            };

            // decode the full blob
            let decoded_blob_data = decode_blob_data(&blob_sidecar.blob)?;

            batcher_transactions_data.push(decoded_blob_data);
        }

        Ok(batcher_transactions_data)
    }

    /// Check if a transaction was sent from the batch sender to the batch inbox.
    #[inline]
    fn is_valid_batcher_transaction(&self, tx: &Transaction) -> bool {
        let batch_sender = ethers::types::Address::from_slice(
            self.config.chain.system_config.batch_sender.as_slice(),
        );
        let batch_inbox =
            ethers::types::Address::from_slice(self.config.chain.batch_inbox.as_slice());
        tx.from == batch_sender && tx.to.map(|to| to == batch_inbox).unwrap_or(false)
    }
}

fn generate_http_provider(url: &str) -> Arc<Provider<RetryClient<Http>>> {
    let client = reqwest::ClientBuilder::new()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let http = Http::new_with_client(Url::parse(url).expect("invalid rpc url"), client);
    let policy = Box::new(HttpRateLimitRetryPolicy);
    let client = RetryClient::new(http, policy, 100, 50);
    Arc::new(Provider::new(client))
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ethers::{
        providers::{Http, Middleware, Provider},
        types::{BlockId, BlockNumber},
    };
    use tokio::sync::mpsc;

    use crate::{
        config::{ChainConfig, Config},
        l1::chain_watcher::InnerWatcher,
    };

    #[tokio::test]
    async fn test_get_batcher_transactions() {
        let Ok(l1_beacon_url) = std::env::var("L1_TEST_BEACON_RPC_URL") else {
            println!("L1_TEST_BEACON_RPC_URL not set; skipping test");
            return;
        };
        let Ok(l1_rpc_url) = std::env::var("L1_TEST_RPC_URL") else {
            println!("L1_TEST_RPC_URL not set; skipping test");
            return;
        };

        let config = Arc::new(Config {
            l1_beacon_url,
            chain: ChainConfig::optimism_sepolia(),
            ..Default::default()
        });

        let l1_provider = Provider::<Http>::try_from(l1_rpc_url).unwrap();
        let l1_block = l1_provider
            .get_block_with_txs(BlockId::Number(BlockNumber::Latest))
            .await
            .unwrap()
            .unwrap();

        let watcher_inner = InnerWatcher::new(config, mpsc::channel(1).0, 0, 0).await;

        let batcher_transactions = watcher_inner
            .get_batcher_transactions(&l1_block)
            .await
            .unwrap();

        batcher_transactions.iter().for_each(|tx| {
            assert!(!tx.is_empty());
        });
    }
}
