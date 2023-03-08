use std::{sync::Arc, vec::IntoIter};

use ethers_core::types::{H256, U64};
use magi::{
    common::Epoch,
    config::{ChainConfig, Config},
    driver::Driver,
    engine::{
        ExecutionPayload, ForkChoiceUpdate, MockEngine, PayloadAttributes, PayloadStatus, Status,
    },
};

#[tokio::test(flavor = "multi_thread")]
async fn test_advance() {
    let next_block_hash = H256::random();

    let config = Arc::new(creat_config());
    let engine = create_engine(next_block_hash, &config);
    let pipeline = create_pipeline();

    let mut driver = Driver::from_internals(engine, pipeline, config).unwrap();
    driver.advance().await.unwrap();

    assert_eq!(driver.safe_head.hash, next_block_hash);
}

fn creat_config() -> Config {
    Config {
        chain: ChainConfig::goerli(),
        l2_rpc_url: None,
        l1_rpc_url: "http://example.com".to_string(),
        engine_api_url: Some(String::new()),
        data_dir: None,
        jwt_secret: None,
    }
}

fn create_pipeline() -> IntoIter<PayloadAttributes> {
    let attr = PayloadAttributes {
        epoch: Some(Epoch::default()),
        ..Default::default()
    };
    let attributes = vec![attr];
    attributes.into_iter()
}

fn create_engine(next_block_hash: H256, config: &Config) -> MockEngine {
    MockEngine {
        forkchoice_updated_payloads_res: ForkChoiceUpdate {
            payload_status: PayloadStatus {
                status: Status::Valid,
                latest_valid_hash: Some(config.chain.l2_genesis.hash),
                validation_error: None,
            },
            payload_id: Some(U64([5])),
        },
        get_payload_res: ExecutionPayload {
            block_hash: next_block_hash,
            ..Default::default()
        },
        new_payload_res: PayloadStatus {
            status: Status::Valid,
            latest_valid_hash: Some(config.chain.l2_genesis.hash),
            validation_error: None,
        },
        forkchoice_updated_res: ForkChoiceUpdate {
            payload_status: PayloadStatus {
                status: Status::Valid,
                latest_valid_hash: Some(next_block_hash),
                validation_error: None,
            },
            payload_id: None,
        },
    }
}
