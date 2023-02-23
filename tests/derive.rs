use std::{cell::RefCell, collections::HashMap, rc::Rc};

use ethers::{
    providers::{Middleware, Provider},
    types::{Block, Transaction, H256},
    utils::keccak256,
};
use eyre::{eyre, Result};

use magi::{
    base_chain::ChainWatcher,
    stages::{
        attributes::{Attributes, PayloadAttributes, UserDeposited},
        batcher_transactions::BatcherTransactions,
        batches::Batches,
        channels::Channels,
        Stage,
    },
};

#[tokio::test]
async fn test_attributes_match() {
    let start_epoch = 8494058;
    let start_block = 5503464;
    let num = 100;

    let attributes = get_attributes(start_epoch, num).await.unwrap();

    let provider =
        Provider::try_from("https://opt-goerli.g.alchemy.com/v2/Olu7jiUDhtHf1iWldKzbBXGB6ImGs0XM")
            .unwrap();

    for i in 0..num {
        let block_num = start_block + i as u64;
        let block = provider.get_block(block_num).await.unwrap().unwrap();
        let expected_hashes = block.transactions;

        let hashes = attributes[i]
            .transactions
            .iter()
            .map(|tx| H256::from_slice(&keccak256(&tx.0)))
            .collect::<Vec<_>>();

        assert_eq!(hashes, expected_hashes);
    }
}

async fn get_attributes(start_epoch: u64, num: usize) -> Result<Vec<PayloadAttributes>> {
    let mut chain_watcher = ChainWatcher::new(start_epoch);

    let blocks = Rc::new(RefCell::new(HashMap::<H256, Block<Transaction>>::new()));
    let deposits = Rc::new(RefCell::new(HashMap::<u64, Vec<UserDeposited>>::new()));

    let batcher_txs = BatcherTransactions::new();
    let channels = Channels::new(batcher_txs.clone());
    let batches = Batches::new(channels.clone(), start_epoch);
    let attributes = Attributes::new(batches.clone(), blocks.clone(), deposits.clone());

    let mut payloads = Vec::new();

    while let Some(data) = chain_watcher.tx_receiver.recv().await {
        while let Ok(block) = chain_watcher.block_receiver.try_recv() {
            blocks.borrow_mut().insert(block.hash.unwrap(), block);
        }

        while let Ok(deposit) = chain_watcher.deposit_receiver.try_recv() {
            let mut deposits = deposits.borrow_mut();
            let deposits_for_block = deposits.get_mut(&deposit.base_block_num);

            if let Some(deposits_for_block) = deposits_for_block {
                deposits_for_block.push(deposit);
            } else {
                deposits.insert(deposit.base_block_num, vec![deposit]);
            }
        }

        batcher_txs.borrow_mut().push_data(data)?;

        while let Some(payload_attributes) = attributes.borrow_mut().next()? {
            if payloads.len() == num {
                return Ok(payloads);
            }

            payloads.push(payload_attributes);
        }
    }

    Err(eyre!("unreachable"))
}
