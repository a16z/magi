use std::{str::FromStr, time::Duration};

use ethers::{
    abi::{decode, ParamType},
    providers::{Middleware, Provider},
    types::{Address, Block, Filter, Transaction, H256, U256},
};
use tokio::{
    spawn,
    sync::mpsc::{channel, Receiver},
    time::sleep,
};

use crate::stages::attributes::UserDeposited;

type BatcherTransactionData = Vec<u8>;

pub fn chain_watcher(
    start_block: u64,
) -> (
    Receiver<BatcherTransactionData>,
    Receiver<Block<Transaction>>,
    Receiver<UserDeposited>,
) {
    let (batcher_tx_sender, batcher_tx_receiver) = channel(1000);
    let (block_sender, block_receiver) = channel(1000);
    let (deposit_sender, deposit_receiver) = channel(1000);

    spawn(async move {
        let url = "https://eth-goerli.g.alchemy.com/v2/a--NIcyeycPntQX42kunxUIVkg6_ekYc";
        let provider = Provider::try_from(url).unwrap();

        let batch_sender = Address::from_str("0x7431310e026b69bfc676c0013e12a1a11411eec9").unwrap();
        let batch_inbox = Address::from_str("0xff00000000000000000000000000000000000420").unwrap();

        let deposit_contract =
            Address::from_str("0x5b47E1A08Ea6d985D6649300584e6722Ec4B1383").unwrap();
        let deposit_topic =
            H256::from_str("0xb3813568d9991fc951961fcb4c784893574240a28925604d09fc577c55bb7c32")
                .unwrap();

        let deposit_filter = Filter::new()
            .address(deposit_contract)
            .topic0(deposit_topic);

        let mut block_num = start_block;

        loop {
            let block = provider
                .get_block_with_txs(block_num)
                .await
                .unwrap()
                .unwrap();

            let batcher_txs = block.transactions.clone().into_iter().filter(|tx| {
                tx.from == batch_sender && tx.to.map(|to| to == batch_inbox).unwrap_or(false)
            });

            let block_hash = block.hash.unwrap();

            // blocks must be sent first to prevent stage from executing on a
            // batch that we do not have the block for yet
            block_sender.send(block).await.unwrap();

            let filter = deposit_filter
                .clone()
                .from_block(block_num)
                .to_block(block_num);

            let logs = provider.get_logs(&filter).await.unwrap();

            let deposits = logs.into_iter().map(|log| {
                let opaque_data = decode(&[ParamType::Bytes], &log.data).unwrap()[0]
                    .clone()
                    .into_bytes()
                    .unwrap();

                let from = Address::try_from(log.topics[1]).unwrap();
                let to = Address::try_from(log.topics[2]).unwrap();
                let mint = U256::from_big_endian(&opaque_data[0..32]);
                let value = U256::from_big_endian(&opaque_data[32..64]);
                let gas = u64::from_be_bytes(opaque_data[64..72].try_into().unwrap());
                let is_creation = opaque_data[72] != 0;
                let data = opaque_data[73..].to_vec();

                let base_block_num = block_num;
                let base_block_hash = block_hash;
                let log_index = log.log_index.unwrap();

                UserDeposited {
                    from,
                    to,
                    mint,
                    value,
                    gas,
                    is_creation,
                    data,
                    base_block_num,
                    base_block_hash,
                    log_index,
                }
            });

            for deposit in deposits {
                deposit_sender.send(deposit).await.unwrap();
            }

            for batcher_tx in batcher_txs {
                batcher_tx_sender
                    .send(batcher_tx.input.to_vec())
                    .await
                    .unwrap();
            }

            block_num += 1;

            sleep(Duration::from_millis(250)).await;
        }
    });

    (batcher_tx_receiver, block_receiver, deposit_receiver)
}
