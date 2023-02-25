use ethers_core::types::*;
use magi::backend::prelude::*;

#[test]
fn test_backend_db() {
    let mut db = Database::new("/tmp/magi");
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
    let mut db = Database::new("/tmp/magi");
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
    let mut db = Database::new("/tmp/magi");
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
