use std::{cell::RefCell, rc::Rc, sync::Arc};

use ethers_core::{types::H256, utils::keccak256};
use ethers_providers::{Middleware, Provider};

use magi::{
    common::RawTransaction,
    config::{ChainConfig, Config},
    derive::{state::State, Pipeline},
    l1::ChainWatcher,
    telemetry,
};

#[tokio::test(flavor = "multi_thread")]
async fn test_attributes_match() {
    telemetry::init(true).unwrap();

    let rpc = "https://eth-goerli.g.alchemy.com/v2/a--NIcyeycPntQX42kunxUIVkg6_ekYc";

    let config = Arc::new(Config {
        l1_rpc: rpc.to_string(),
        engine_url: String::new(),
        jwt_secret: String::new(),
        db_location: None,
        chain: ChainConfig::goerli(),
    });

    let mut chain_watcher =
        ChainWatcher::new(config.chain.l1_start_epoch.number, config.clone()).unwrap();
    let tx_recv = chain_watcher.take_tx_receiver().unwrap();
    let state = Rc::new(RefCell::new(State::new(
        config.chain.l2_genesis,
        config.chain.l1_start_epoch,
        chain_watcher,
    )));

    let mut pipeline = Pipeline::new(state, tx_recv, config.clone()).unwrap();

    if let Some(payload) = pipeline.next() {
        let hashes = get_tx_hashes(&payload.transactions.unwrap());
        let expected_hashes = get_expected_hashes(config.chain.l2_genesis.number + 1).await;

        assert_eq!(hashes, expected_hashes);
    }
}

async fn get_expected_hashes(block_num: u64) -> Vec<H256> {
    let provider =
        Provider::try_from("https://opt-goerli.g.alchemy.com/v2/Olu7jiUDhtHf1iWldKzbBXGB6ImGs0XM")
            .unwrap();

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
