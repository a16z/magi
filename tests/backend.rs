use std::str::FromStr;

use ethers_core::types::*;
use magi::backend::prelude::*;

#[test]
fn test_backend_db() {
    let mut db = Database::new("/tmp/magi", "optimism-goerli");
    let block = ConstructedBlock {
        hash: Some(BlockHash::from([1; 32])),
        ..Default::default()
    };
    db.write_block(block.clone()).unwrap();
    let read_block = db.read_block(block.hash.unwrap()).unwrap();
    assert_eq!(block, read_block);
    db.clear().unwrap();
}

#[test]
fn test_db_full_block() {
    let mut db = Database::new("/tmp/magi", "optimism-goerli");
    let mut block = ConstructedBlock {
        hash: Some(BlockHash::from([1; 32])),
        number: U64::one(),
        parent_hash: BlockHash::from([2; 32]),
        timestamp: 1000,
        ..Default::default()
    };
    let tx = Transaction {
        hash: BlockHash::from([3; 32]),
        nonce: U256::one(),
        from: Address::from([4; 20]),
        to: Some(Address::from([5; 20])),
        value: U256::one(),
        ..Default::default()
    };
    block.transactions = vec![tx];
    db.write_block(block.clone()).unwrap();
    let read_block = db.read_block(block.hash.unwrap()).unwrap();
    assert_eq!(block, read_block);
    db.clear().unwrap();
}

#[test]
fn test_db_missing_block_hash() {
    let mut db = Database::new("/tmp/magi", "optimism-goerli");
    let mut block = ConstructedBlock {
        hash: None,
        number: U64::from(69),
        parent_hash: BlockHash::from([2; 32]),
        timestamp: 1000,
        ..Default::default()
    };
    let tx = Transaction {
        hash: BlockHash::from([3; 32]),
        nonce: U256::one(),
        from: Address::from([4; 20]),
        to: Some(Address::from([5; 20])),
        value: U256::one(),
        ..Default::default()
    };
    block.transactions = vec![tx];
    db.write_block(block.clone()).unwrap();
    let read_block = db.read_block(block.number).unwrap();
    assert_eq!(block, read_block);
    db.clear().unwrap();
}

#[test]
fn test_db_fetch_by_timestamp() {
    let mut db = Database::new("/tmp/magi", "optimism-goerli");
    let block = ConstructedBlock {
        hash: None,
        number: U64::from(69),
        parent_hash: BlockHash::from([2; 32]),
        timestamp: 1000,
        ..Default::default()
    };
    let block2 = ConstructedBlock {
        hash: None,
        number: U64::from(70),
        parent_hash: BlockHash::from([2; 32]),
        timestamp: 1000,
        ..Default::default()
    };
    db.write_block(block.clone()).unwrap();
    db.write_block(block2.clone()).unwrap();
    let blocks = db.blocks_by_timestamp(1000);
    assert_eq!(vec![block, block2], blocks);
    db.clear().unwrap();
}

#[test]
fn test_db_fetch_by_origin_hash() {
    let block_hash =
        BlockHash::from_str("0x91e7616d8588b3ff63aea5cf406f37f62bb5767a6c59551e0b163be475de145c")
            .unwrap();
    let mut db = Database::new("/tmp/magi", "optimism-goerli");
    let block = ConstructedBlock {
        hash: None,
        number: U64::from(69),
        parent_hash: BlockHash::from([2; 32]),
        timestamp: 1000,
        l1_origin_block_hash: Some(block_hash),
        ..Default::default()
    };
    let block2 = ConstructedBlock {
        hash: None,
        number: U64::from(70),
        parent_hash: BlockHash::from([2; 32]),
        timestamp: 2000,
        l1_origin_block_hash: Some(block_hash),
        ..Default::default()
    };
    db.write_block(block.clone()).unwrap();
    db.write_block(block2.clone()).unwrap();
    let blocks = db.blocks_by_origin_hash(block_hash);
    assert_eq!(vec![block, block2], blocks);
    db.clear().unwrap();
}

#[test]
fn test_db_fetch_by_origin_number() {
    let block_number = U64::from(16706439);
    let mut db = Database::new("/tmp/magi", "optimism-goerli");
    let block = ConstructedBlock {
        hash: None,
        number: U64::from(69),
        parent_hash: BlockHash::from([2; 32]),
        timestamp: 1000,
        l1_origin_block_number: Some(block_number),
        ..Default::default()
    };
    let block2 = ConstructedBlock {
        hash: None,
        number: U64::from(70),
        parent_hash: BlockHash::from([2; 32]),
        timestamp: 2000,
        l1_origin_block_number: Some(block_number),
        ..Default::default()
    };
    db.write_block(block.clone()).unwrap();
    db.write_block(block2.clone()).unwrap();
    let blocks = db.blocks_by_origin_number(block_number);
    assert_eq!(vec![block, block2], blocks);
    db.clear().unwrap();
}

#[test]
fn test_db_fetch_by_transaction_hash() {
    let mut db = Database::new("/tmp/magi", "optimism-goerli");
    let tx_hash = TxHash::from([3; 32]);
    let tx = Transaction {
        hash: tx_hash,
        nonce: U256::one(),
        from: Address::from([4; 20]),
        to: Some(Address::from([5; 20])),
        value: U256::one(),
        ..Default::default()
    };
    let block = ConstructedBlock {
        hash: None,
        number: U64::from(69),
        parent_hash: BlockHash::from([2; 32]),
        timestamp: 1000,
        transactions: vec![tx],
        ..Default::default()
    };
    db.write_block(block.clone()).unwrap();
    let fetched = db.block_by_tx_hash(tx_hash);
    assert_eq!(Some(block), fetched);
    db.clear().unwrap();
}
