use std::sync::{mpsc, Arc, RwLock};

use eyre::Result;

use crate::{config::ChainConfig, engine::PayloadAttributes, l1::L1Info, types::common::Epoch};

use self::{
    stages::{
        attributes::Attributes,
        batcher_transactions::{BatcherTransactionMessage, BatcherTransactions},
        batches::Batches,
        channels::Channels,
    },
    state::State,
};

pub mod stages;
pub mod state;

mod purgeable;
pub use purgeable::PurgeableIterator;

pub struct Pipeline {
    batcher_transaction_sender: mpsc::Sender<BatcherTransactionMessage>,
    attributes: Attributes,
    pending_attributes: Option<PayloadAttributes>,
}

impl Iterator for Pipeline {
    type Item = PayloadAttributes;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pending_attributes.is_some() {
            self.pending_attributes.take()
        } else {
            self.attributes.next()
        }
    }
}

impl Pipeline {
    pub fn new(
        state: Arc<RwLock<State>>,
        chain: Arc<ChainConfig>,
        seq: u64,
        unsafe_seq: u64,
    ) -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        let batcher_transactions = BatcherTransactions::new(rx);
        let channels = Channels::new(
            batcher_transactions,
            chain.max_channel_size,
            chain.channel_timeout,
        );
        let batches = Batches::new(channels, state.clone(), Arc::clone(&chain));
        let attributes = Attributes::new(
            Box::new(batches),
            state,
            chain.regolith_time,
            seq,
            unsafe_seq,
            chain.canyon_time,
        );

        Ok(Self {
            batcher_transaction_sender: tx,
            attributes,
            pending_attributes: None,
        })
    }

    pub fn push_batcher_transactions(&self, txs: Vec<Vec<u8>>, l1_origin: u64) -> Result<()> {
        self.batcher_transaction_sender
            .send(BatcherTransactionMessage { txs, l1_origin })?;
        Ok(())
    }

    pub fn derive_attributes_for_next_block(
        &mut self,
        epoch: Epoch,
        l1_info: &L1Info,
        block_timestamp: u64,
    ) -> PayloadAttributes {
        self.attributes.update_unsafe_seq_num(&epoch);
        self.attributes
            .derive_attributes_for_next_block(epoch, l1_info, block_timestamp)
    }

    pub fn peek(&mut self) -> Option<&PayloadAttributes> {
        if self.pending_attributes.is_none() {
            let next_attributes = self.next();
            self.pending_attributes = next_attributes;
        }

        self.pending_attributes.as_ref()
    }

    pub fn purge(&mut self) -> Result<()> {
        self.attributes.purge();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, RwLock};

    use ethers::{
        providers::{Middleware, Provider},
        types::H256,
        utils::keccak256,
    };

    use crate::{
        config::{ChainConfig, Config},
        derive::*,
        l1::{BlockUpdate, ChainWatcher},
        types::attributes::RawTransaction,
    };

    #[tokio::test(flavor = "multi_thread")]
    async fn test_attributes_match() {
        let rpc_env = std::env::var("L1_TEST_RPC_URL");
        let l2_rpc_env = std::env::var("L2_TEST_RPC_URL");
        let (rpc, l2_rpc) = match (rpc_env, l2_rpc_env) {
            (Ok(rpc), Ok(l2_rpc)) => (rpc, l2_rpc),
            (rpc_env, l2_rpc_env) => {
                eprintln!("Test ignored: `test_attributes_match`, rpc: {rpc_env:?}, l2_rpc: {l2_rpc_env:?}");
                return;
            }
        };

        let provider = Provider::try_from(&l2_rpc).unwrap();
        let config = Arc::new(Config {
            chain: Arc::new(ChainConfig::optimism_goerli()),
            l1_rpc_url: rpc,
            l2_rpc_url: l2_rpc.clone(),
            rpc_port: 9545,
            ..Config::default()
        });

        let mut chain_watcher = ChainWatcher::new(
            config.chain.genesis.l1.number,
            config.chain.genesis.l2.number,
            config.clone(),
        )
        .unwrap();

        chain_watcher.start().unwrap();

        let state = Arc::new(RwLock::new(
            State::new(
                config.chain.l2_genesis(),
                config.chain.l1_start_epoch(),
                config.chain.l2_genesis(),
                config.chain.l1_start_epoch(),
                &provider,
                Arc::clone(&config.chain),
            )
            .await,
        ));

        let mut pipeline = Pipeline::new(state.clone(), Arc::clone(&config.chain), 0, 0).unwrap();

        chain_watcher.recv_from_channel().await.unwrap();
        chain_watcher.recv_from_channel().await.unwrap();
        chain_watcher.recv_from_channel().await.unwrap();
        let update = chain_watcher.recv_from_channel().await.unwrap();

        let l1_info = match update {
            BlockUpdate::NewBlock(block) => *block,
            _ => panic!("wrong update type"),
        };

        pipeline
            .push_batcher_transactions(
                l1_info.batcher_transactions.clone(),
                l1_info.block_info.number,
            )
            .unwrap();

        state.write().unwrap().update_l1_info(l1_info);

        if let Some(payload) = pipeline.next() {
            let hashes = get_tx_hashes(&payload.transactions.unwrap());
            let expected_hashes =
                get_expected_hashes(config.chain.genesis.l2.number + 1, &l2_rpc).await;

            assert_eq!(hashes, expected_hashes);
        }
    }

    async fn get_expected_hashes(block_num: u64, l2_rpc: &str) -> Vec<H256> {
        let provider = Provider::try_from(l2_rpc).unwrap();

        provider
            .get_block(block_num)
            .await
            .unwrap()
            .unwrap()
            .transactions
    }

    fn get_tx_hashes(txs: &[RawTransaction]) -> Vec<H256> {
        txs.iter()
            .map(|tx| H256::from_slice(&keccak256(&tx.0)))
            .collect()
    }
}
