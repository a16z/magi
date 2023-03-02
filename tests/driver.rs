use std::{sync::Arc, vec::IntoIter};

use ethers_core::types::{H256, U64};
use magi::{
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

    let mut driver = Driver::new(engine, pipeline, config);
    driver.advance().await.unwrap();

    assert_eq!(driver.head_block_hash, next_block_hash);
    assert_eq!(driver.safe_block_hash, next_block_hash);
    assert_eq!(driver.finalized_hash, H256::zero());
}

fn creat_config() -> Config {
    Config {
        chain: ChainConfig::goerli(),
        l2_rpc: None,
        l1_rpc: String::new(),
        max_channels: 100_000_000,
        max_timeout: 100,
    }
}

fn create_pipeline() -> IntoIter<PayloadAttributes> {
    let attributes = vec![PayloadAttributes::default()];
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
