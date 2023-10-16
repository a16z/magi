use std::sync::{mpsc, Arc, RwLock};

use eyre::Result;

use crate::specular::stages::{
    batcher_transactions::SpecularBatcherTransactions, batches::SpecularBatches,
};
use crate::{config::Config, engine::PayloadAttributes};

use self::{
    stages::{
        attributes::Attributes,
        batcher_transactions::{BatcherTransactionMessage, BatcherTransactions},
        batches::{Batch, Batches},
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
    pub fn new(state: Arc<RwLock<State>>, config: Arc<Config>, seq: u64) -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        let batch_iter: Box<dyn PurgeableIterator<Item = Batch>> =
            if config.chain.meta.enable_full_derivation {
                let batcher_transactions = BatcherTransactions::new(rx);
                let channels = Channels::new(batcher_transactions, config.clone());
                let batches = Batches::new(channels, state.clone(), config.clone());
                Box::new(batches)
            } else {
                let batcher_transactions = SpecularBatcherTransactions::new(rx);
                let batches =
                    SpecularBatches::new(batcher_transactions, state.clone(), config.clone());
                Box::new(batches)
            };
        let attributes = Attributes::new(batch_iter, state, config, seq);

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
    use std::{
        env,
        sync::{Arc, RwLock},
    };

    use ethers::{
        providers::{Middleware, Provider},
        types::H256,
        utils::keccak256,
    };

    use crate::{
        common::RawTransaction,
        config::{ChainConfig, Config},
        derive::*,
        l1::{BlockUpdate, ChainWatcher},
    };

    #[tokio::test(flavor = "multi_thread")]
    async fn test_attributes_match() {
        if std::env::var("L1_TEST_RPC_URL").is_ok() && std::env::var("L2_TEST_RPC_URL").is_ok() {
            let rpc = env::var("L1_TEST_RPC_URL").unwrap();
            let l2_rpc = env::var("L2_TEST_RPC_URL").unwrap();

            let config = Arc::new(Config {
                l1_rpc_url: rpc.to_string(),
                l2_rpc_url: l2_rpc.to_string(),
                chain: ChainConfig::optimism_goerli(),
                l2_engine_url: String::new(),
                jwt_secret: String::new(),
                checkpoint_sync_url: None,
                rpc_port: 9545,
                devnet: false,
                local_sequencer: Default::default(),
            });

            let mut chain_watcher = ChainWatcher::new(
                config.chain.l1_start_epoch.number,
                config.chain.l2_genesis.number,
                config.clone(),
            )
            .unwrap();

            chain_watcher.start().unwrap();

            let state = Arc::new(RwLock::new(State::new(
                config.chain.l2_genesis,
                config.chain.l1_start_epoch,
                config.clone(),
            )));

            let mut pipeline = Pipeline::new(state.clone(), config.clone(), 0).unwrap();

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
                let expected_hashes = get_expected_hashes(config.chain.l2_genesis.number + 1).await;

                assert_eq!(hashes, expected_hashes);
            }
        }
    }

    async fn get_expected_hashes(block_num: u64) -> Vec<H256> {
        let provider = Provider::try_from(env::var("L2_TEST_RPC_URL").unwrap()).unwrap();

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
