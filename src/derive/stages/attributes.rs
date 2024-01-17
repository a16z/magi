use std::sync::{Arc, RwLock};

use ethers::types::{H256, U64};
use ethers::utils::rlp::Encodable;

use crate::config::SystemAccounts;
use crate::derive::state::State;
use crate::derive::PurgeableIterator;
use crate::engine::PayloadAttributes;
use crate::l1::L1Info;
use crate::types::attributes::{AttributesDeposited, DepositedTransaction};
use crate::types::{attributes::RawTransaction, common::Epoch};

use super::block_input::BlockInput;

pub struct Attributes {
    block_input_iter: Box<dyn PurgeableIterator<Item = BlockInput<u64>>>,
    state: Arc<RwLock<State>>,
    seq_num: u64,
    epoch_hash: H256,
    regolith_time: u64,
    unsafe_seq_num: u64,
    canyon_time: u64,
}

impl Iterator for Attributes {
    type Item = PayloadAttributes;

    fn next(&mut self) -> Option<Self::Item> {
        self.block_input_iter
            .next()
            .map(|input| input.with_full_epoch(&self.state).unwrap())
            .map(|batch| {
                self.update_sequence_number(batch.epoch.hash);
                self.derive_attributes(batch)
            })
    }
}

impl PurgeableIterator for Attributes {
    fn purge(&mut self) {
        self.block_input_iter.purge();
        self.seq_num = 0;
        self.epoch_hash = self.state.read().unwrap().safe_epoch.hash;
    }
}

impl Attributes {
    pub fn new(
        block_input_iter: Box<dyn PurgeableIterator<Item = BlockInput<u64>>>,
        state: Arc<RwLock<State>>,
        regolith_time: u64,
        seq_num: u64,
        unsafe_seq_num: u64,
        canyon_time: u64,
    ) -> Self {
        let epoch_hash = state.read().expect("lock poisoned").safe_epoch.hash;

        Self {
            block_input_iter,
            state,
            seq_num,
            epoch_hash,
            regolith_time,
            unsafe_seq_num,
            canyon_time,
        }
    }

    pub fn derive_attributes_for_next_block(
        &mut self,
        epoch: Epoch,
        l1_info: &L1Info,
        block_timestamp: u64,
    ) -> PayloadAttributes {
        self.derive_attributes_internal(
            epoch,
            l1_info,
            block_timestamp,
            vec![],
            self.unsafe_seq_num,
            Some(epoch.number),
        )
    }

    fn derive_attributes(&self, input: BlockInput<Epoch>) -> PayloadAttributes {
        tracing::debug!("attributes derived from block {}", input.epoch.number);
        tracing::debug!("batch epoch hash {:?}", input.epoch.hash);

        let state = self.state.read().unwrap();
        let l1_info = state.l1_info_by_hash(input.epoch.hash).unwrap();

        let epoch = Epoch {
            number: input.epoch.number,
            hash: input.epoch.hash,
            timestamp: l1_info.block_info.timestamp,
        };

        self.derive_attributes_internal(
            epoch,
            l1_info,
            input.timestamp,
            input.transactions,
            self.seq_num,
            Some(input.l1_inclusion_block),
        )
    }

    fn derive_attributes_internal(
        &self,
        epoch: Epoch,
        l1_info: &L1Info,
        timestamp: u64,
        transactions: Vec<RawTransaction>,
        seq_number: u64,
        l1_inclusion_block: Option<u64>,
    ) -> PayloadAttributes {
        let transactions = Some(self.derive_transactions(
            timestamp,
            transactions,
            l1_info,
            epoch.hash,
            seq_number,
        ));
        let suggested_fee_recipient = SystemAccounts::default().fee_vault;

        let withdrawals = if timestamp >= self.canyon_time {
            Some(Vec::new())
        } else {
            None
        };

        PayloadAttributes {
            timestamp: U64([timestamp]),
            prev_randao: l1_info.block_info.mix_hash,
            suggested_fee_recipient,
            transactions,
            no_tx_pool: false,
            gas_limit: U64([l1_info.system_config.gas_limit]),
            withdrawals,
            epoch: Some(epoch),
            l1_inclusion_block,
            seq_number: Some(seq_number),
        }
    }

    fn derive_transactions(
        &self,
        timestamp: u64,
        batch_txs: Vec<RawTransaction>,
        l1_info: &L1Info,
        epoch_hash: H256,
        seq: u64,
    ) -> Vec<RawTransaction> {
        let mut transactions = Vec::new();

        let attributes_tx = self.derive_attributes_deposited(l1_info, timestamp, seq);
        transactions.push(attributes_tx);

        if seq == 0 {
            let mut user_deposited_txs = self.derive_user_deposited(epoch_hash);
            transactions.append(&mut user_deposited_txs);
        }

        let mut rest = batch_txs;
        transactions.append(&mut rest);

        transactions
    }

    fn derive_attributes_deposited(
        &self,
        l1_info: &L1Info,
        batch_timestamp: u64,
        seq: u64,
    ) -> RawTransaction {
        let attributes_deposited =
            AttributesDeposited::from_block_info(l1_info, seq, batch_timestamp, self.regolith_time);
        let attributes_tx = DepositedTransaction::from(attributes_deposited);
        RawTransaction(attributes_tx.rlp_bytes().to_vec())
    }

    fn derive_user_deposited(&self, epoch_hash: H256) -> Vec<RawTransaction> {
        let state = self.state.read().expect("lock poisoned");
        state
            .l1_info_by_hash(epoch_hash)
            .map(|info| &info.user_deposits)
            .map(|deposits| {
                deposits
                    .iter()
                    .map(|deposit| {
                        let tx = DepositedTransaction::from(deposit.clone());
                        RawTransaction(tx.rlp_bytes().to_vec())
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn update_sequence_number(&mut self, batch_epoch_hash: H256) {
        if self.epoch_hash != batch_epoch_hash {
            self.seq_num = 0;
        } else {
            self.seq_num += 1;
        }

        self.epoch_hash = batch_epoch_hash;
    }

    pub(crate) fn update_unsafe_seq_num(&mut self, epoch: &Epoch) {
        let unsafe_epoch = self.state.read().expect("lock poisoned").unsafe_epoch;

        if unsafe_epoch.hash != epoch.hash {
            self.unsafe_seq_num = 0;
        } else {
            self.unsafe_seq_num += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{ChainConfig, SystemAccounts};
    use crate::derive::stages::attributes::Attributes;
    use crate::derive::stages::batcher_transactions::BatcherTransactions;
    use crate::derive::stages::batches::Batches;
    use crate::derive::stages::channels::Channels;
    use crate::derive::state::State;
    use crate::l1::{L1BlockInfo, L1Info};
    use crate::types::attributes::{DepositedTransaction, UserDeposited};
    use crate::types::common::{BlockInfo, Epoch};
    use ethers::prelude::Provider;
    use ethers::types::{Address, H256, U256, U64};
    use ethers::utils::rlp::Rlp;
    use std::sync::{mpsc, Arc, RwLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn test_derive_attributes_for_next_block_same_epoch() {
        let l2_rpc = match std::env::var("L2_TEST_RPC_URL") {
            Ok(l2_rpc) => l2_rpc,
            l2_rpc_res => {
                eprintln!(
                    "Test ignored: `test_derive_attributes_for_next_block_same_epoch`, l2_rpc: {l2_rpc_res:?}"
                );
                return;
            }
        };

        // Let's say we just started, the unsafe/safe/finalized heads are same.
        // New block would be generated in the same epoch.
        // Prepare required state.
        let chain = Arc::new(ChainConfig::optimism_goerli());

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let block_info = BlockInfo {
            hash: H256::random(),
            number: 0,
            parent_hash: H256::random(),
            timestamp: now,
        };
        let epoch = Epoch {
            number: 0,
            hash: H256::random(),
            timestamp: now,
        };

        let l1_info = L1Info {
            block_info: L1BlockInfo {
                number: epoch.number,
                hash: epoch.hash,
                timestamp: epoch.timestamp,
                parent_hash: H256::zero(),
                base_fee: U256::zero(),
                mix_hash: H256::zero(),
            },
            system_config: chain.genesis.system_config,
            user_deposits: vec![],
            batcher_transactions: vec![],
            finalized: false,
        };

        let provider = Provider::try_from(l2_rpc).unwrap();
        let state = Arc::new(RwLock::new(
            State::new(
                block_info,
                epoch,
                block_info,
                epoch,
                &provider,
                Arc::clone(&chain),
            )
            .await,
        ));

        state.write().unwrap().update_l1_info(l1_info.clone());

        let (_tx, rx) = mpsc::channel();
        let batcher_transactions = BatcherTransactions::new(rx);
        let channels = Channels::new(
            batcher_transactions,
            chain.max_channel_size,
            chain.channel_timeout,
        );
        let batches = Batches::new(channels, state.clone(), Arc::clone(&chain));

        let mut attributes =
            Attributes::new(Box::new(batches), state, chain.regolith_time, 0, 0, 0);
        let attrs = attributes.derive_attributes_for_next_block(epoch, &l1_info, now + 2);

        // Check fields.
        assert_eq!(attrs.timestamp, (now + 2).into(), "timestamp doesn't match");
        assert_eq!(
            attrs.prev_randao, l1_info.block_info.mix_hash,
            "prev rando doesn't match"
        );
        assert_eq!(
            attrs.suggested_fee_recipient,
            SystemAccounts::default().fee_vault,
            "fee recipient doesn't match"
        );
        assert!(!attrs.no_tx_pool, "no tx pool doesn't match");
        assert_eq!(
            attrs.gas_limit,
            U64([l1_info.system_config.gas_limit]),
            "gas limit doesn't match"
        );
        assert!(attrs.epoch.is_some(), "epoch missed");
        assert_eq!(attrs.epoch.unwrap(), epoch, "epoch doesn't match");
        assert!(
            attrs.l1_inclusion_block.is_some(),
            "l1 inclusion block missed"
        );
        assert_eq!(
            attrs.l1_inclusion_block.unwrap(),
            0,
            "wrong l1 inclusion block"
        );
        assert!(attrs.seq_number.is_some(), "seq number missed");
        assert_eq!(attrs.seq_number.unwrap(), 1, "wrong sequence number");

        // Check transactions.
        assert!(attrs.transactions.is_some(), "missed transactions");
        let transactions = attrs.transactions.unwrap();
        assert_eq!(transactions.len(), 1, "wrong transactions length");

        let (deposited_epoch, seq_number) =
            transactions.first().unwrap().derive_unsafe_epoch().unwrap();
        assert_eq!(
            deposited_epoch, epoch,
            "wrong epoch in deposited transaction"
        );
        assert_eq!(attrs.seq_number.unwrap(), seq_number);
    }

    #[tokio::test]
    async fn test_derive_attributes_for_next_block_new_epoch() {
        let l2_rpc = match std::env::var("L2_TEST_RPC_URL") {
            Ok(l2_rpc) => l2_rpc,
            l2_rpc_res => {
                eprintln!(
                    "Test ignored: `test_derive_attributes_for_next_block_new_epoch`, l2_rpc: {l2_rpc_res:?}"
                );
                return;
            }
        };

        // Now let's say we will generate a payload in a new epoch.
        // Must contain deposit transactions.
        let chain = Arc::new(ChainConfig::optimism_goerli());

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let block_info = BlockInfo {
            hash: H256::random(),
            number: 0,
            parent_hash: H256::random(),
            timestamp: now,
        };
        let epoch = Epoch {
            number: 0,
            hash: H256::random(),
            timestamp: now,
        };

        let l1_block_num = 1;
        let l1_block_hash = H256::random();

        let new_epoch = Epoch {
            number: l1_block_num,
            hash: l1_block_hash,
            timestamp: now + 2,
        };

        let user_deposited = UserDeposited {
            from: Address::random(),
            to: Address::random(),
            mint: U256::zero(),
            value: U256::from(10),
            gas: 10000,
            is_creation: false,
            data: vec![],
            l1_block_num,
            l1_block_hash,
            log_index: U256::zero(),
        };

        let l1_info = L1Info {
            block_info: L1BlockInfo {
                number: new_epoch.number,
                hash: new_epoch.hash,
                timestamp: new_epoch.timestamp,
                parent_hash: H256::zero(),
                base_fee: U256::zero(),
                mix_hash: H256::zero(),
            },
            system_config: chain.genesis.system_config,
            user_deposits: vec![user_deposited.clone()],
            batcher_transactions: vec![],
            finalized: false,
        };

        let provider = Provider::try_from(l2_rpc).unwrap();
        let state = Arc::new(RwLock::new(
            State::new(
                block_info,
                epoch,
                block_info,
                epoch,
                &provider,
                Arc::clone(&chain),
            )
            .await,
        ));

        state.write().unwrap().update_l1_info(l1_info.clone());

        let (_tx, rx) = mpsc::channel();
        let batcher_transactions = BatcherTransactions::new(rx);
        let channels = Channels::new(
            batcher_transactions,
            chain.max_channel_size,
            chain.channel_timeout,
        );
        let batches = Batches::new(channels, state.clone(), Arc::clone(&chain));

        let mut attributes =
            Attributes::new(Box::new(batches), state, chain.regolith_time, 0, 0, 0);
        let attrs = attributes.derive_attributes_for_next_block(new_epoch, &l1_info, now + 2);

        // Check fields.
        assert_eq!(attrs.timestamp, (now + 2).into(), "timestamp doesn't match");
        assert_eq!(
            attrs.prev_randao, l1_info.block_info.mix_hash,
            "prev rando doesn't match"
        );
        assert_eq!(
            attrs.suggested_fee_recipient,
            SystemAccounts::default().fee_vault,
            "fee recipient doesn't match"
        );
        assert!(!attrs.no_tx_pool, "no tx pool doesn't match");
        assert_eq!(
            attrs.gas_limit,
            U64([l1_info.system_config.gas_limit]),
            "gas limit doesn't match"
        );
        assert!(attrs.epoch.is_some(), "epoch missed");
        assert_eq!(attrs.epoch.unwrap(), new_epoch, "epoch doesn't match");
        assert!(
            attrs.l1_inclusion_block.is_some(),
            "l1 inclusion block missed"
        );
        assert_eq!(
            attrs.l1_inclusion_block.unwrap(),
            1,
            "wrong l1 inclusion block"
        );
        assert!(attrs.seq_number.is_some(), "seq number missed");
        assert_eq!(attrs.seq_number.unwrap(), 0, "wrong sequence number");

        // Check transactions.
        assert!(attrs.transactions.is_some(), "missed transactions");
        let transactions = attrs.transactions.unwrap();
        assert_eq!(transactions.len(), 2, "wrong transactions length");

        let (deposited_epoch, seq_number) =
            transactions.first().unwrap().derive_unsafe_epoch().unwrap();
        assert_eq!(
            deposited_epoch, new_epoch,
            "wrong epoch in deposited transaction"
        );
        assert_eq!(attrs.seq_number.unwrap(), seq_number);

        let deposited_tx_raw = transactions.get(1).unwrap();
        let deposited_tx = Rlp::new(&deposited_tx_raw.0)
            .as_val::<DepositedTransaction>()
            .unwrap();
        let deposited_tx_from = DepositedTransaction::from(user_deposited);
        assert_eq!(
            deposited_tx, deposited_tx_from,
            "transaction with deposit doesn't match"
        );
    }
}
