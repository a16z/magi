use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use ethers::providers::{Middleware, Provider};
use ethers::{types::H256, utils::keccak256};

use magi::{
    common::RawTransaction,
    config::{ChainConfig, Config},
    derive::{state::State, Pipeline},
    l1::{BlockUpdate, ChainWatcher},
    telemetry,
};

#[tokio::test(flavor = "multi_thread")]
async fn test_attributes_match() {
    telemetry::init(true).unwrap();

    let rpc = "https://eth-goerli.g.alchemy.com/v2/a--NIcyeycPntQX42kunxUIVkg6_ekYc";
    let l2_rpc = "https://opt-goerli.g.alchemy.com/v2/Olu7jiUDhtHf1iWldKzbBXGB6ImGs0XM";

    let config = Arc::new(Config {
        l1_rpc_url: rpc.to_string(),
        l2_rpc_url: l2_rpc.to_string(),
        chain: ChainConfig::optimism_goerli(),
        data_dir: PathBuf::default(),
        l2_engine_url: String::new(),
        jwt_secret: String::new(),
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

    let mut pipeline = Pipeline::new(state.clone(), config.clone()).unwrap();

    chain_watcher.block_update_receiver.recv().unwrap();
    let update = chain_watcher.block_update_receiver.recv().unwrap();

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
