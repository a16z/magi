use std::sync::Arc;

use ethers_core::{types::H256, utils::keccak256};
use ethers_providers::{Middleware, Provider};

use magi::{
    common::RawTransaction,
    config::{ChainConfig, Config},
    derive::Pipeline,
    telemetry,
};

#[tokio::test(flavor = "multi_thread")]
async fn test_attributes_match() {
    telemetry::init(true).unwrap();

    let start_epoch = 8494058;
    let rpc = "https://eth-goerli.g.alchemy.com/v2/a--NIcyeycPntQX42kunxUIVkg6_ekYc";

    let start_block = 5503464;
    let num = 100;

    let config = Arc::new(Config {
        l1_rpc: rpc.to_string(),
        engine_url: String::new(),
        jwt_secret: String::new(),
        db_location: None,
        chain: ChainConfig::goerli(),
    });

    let mut pipeline = Pipeline::new(start_epoch, config).unwrap();

    let mut i = 0;
    while i < num {
        if let Some(payload) = pipeline.next() {
            let hashes = get_tx_hashes(&payload.transactions.unwrap());
            let expected_hashes = get_expected_hashes(start_block + i).await;

            assert_eq!(hashes, expected_hashes);

            i += 1;
        }
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
