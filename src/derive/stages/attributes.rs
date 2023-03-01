use std::iter;
use std::sync::Arc;
use std::{cell::RefCell, collections::HashMap, rc::Rc};

use ethers_core::abi::{decode, encode, ParamType, Token};
use ethers_core::types::{Address, Block, Log, Transaction, H256, U256, U64};
use ethers_core::utils::{keccak256, rlp::Encodable, rlp::RlpStream};

use eyre::Result;

use crate::common::RawTransaction;
use crate::config::{Config, SystemAccounts};
use crate::engine::PayloadAttributes;

use super::batches::{Batch, Batches};

pub struct Attributes {
    prev_stage: Rc<RefCell<Batches>>,
    config: Arc<Config>,
    blocks: Rc<RefCell<HashMap<H256, Block<Transaction>>>>,
    deposits: Rc<RefCell<HashMap<u64, Vec<UserDeposited>>>>,
    sequence_number: u64,
    epoch: u64,
}

impl Iterator for Attributes {
    type Item = PayloadAttributes;

    fn next(&mut self) -> Option<Self::Item> {
        let batch = self.prev_stage.borrow_mut().next()?;
        let payload_attributes = self.derive_attributes(batch);

        Some(payload_attributes)
    }
}

impl Attributes {
    pub fn new(
        prev_stage: Rc<RefCell<Batches>>,
        config: Arc<Config>,
        blocks: Rc<RefCell<HashMap<H256, Block<Transaction>>>>,
        deposits: Rc<RefCell<HashMap<u64, Vec<UserDeposited>>>>,
    ) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            prev_stage,
            blocks,
            deposits,
            sequence_number: 0,
            epoch: config.chain.l1_start_epoch.number,
            config,
        }))
    }

    fn derive_attributes(&mut self, batch: Batch) -> PayloadAttributes {
        tracing::debug!("attributes derived from block {}", batch.epoch_num);
        tracing::debug!("batch epoch hash {:?}", batch.epoch_hash);

        self.update_sequence_number(batch.epoch_num);

        let blocks = self.blocks.borrow();
        let l1_block = blocks.get(&batch.epoch_hash).unwrap();

        let timestamp = U64([batch.timestamp]);
        let prev_randao = l1_block.mix_hash.unwrap();
        let transactions = Some(self.derive_transactions(batch, l1_block));
        let suggested_fee_recipient = SystemAccounts::default().fee_vault;

        PayloadAttributes {
            timestamp,
            prev_randao,
            suggested_fee_recipient,
            transactions,
            no_tx_pool: true,
            gas_limit: U64([25_000_000]),
        }
    }

    fn derive_transactions(
        &self,
        batch: Batch,
        l1_block: &Block<Transaction>,
    ) -> Vec<RawTransaction> {
        let mut transactions = Vec::new();

        let attributes_tx = self.derive_attributes_deposited(l1_block);
        transactions.push(attributes_tx);

        if self.sequence_number == 0 {
            let mut user_deposited_txs = self.derive_user_deposited();
            transactions.append(&mut user_deposited_txs);
        }

        let mut rest = batch.transactions;
        transactions.append(&mut rest);

        transactions
    }

    fn derive_attributes_deposited(&self, l1_block: &Block<Transaction>) -> RawTransaction {
        let seq = self.sequence_number;
        let batch_sender = self.config.chain.batch_sender;
        let attributes_deposited = AttributesDeposited::from_block(l1_block, seq, &batch_sender);
        let attributes_tx = DepositedTransaction::from(attributes_deposited);
        RawTransaction(attributes_tx.rlp_bytes().to_vec())
    }

    fn derive_user_deposited(&self) -> Vec<RawTransaction> {
        let deposits = self.deposits.borrow();
        let deposits = deposits.get(&self.epoch);
        deposits
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

    fn update_sequence_number(&mut self, batch_epoch: u64) {
        if self.epoch != batch_epoch {
            self.sequence_number = 0;
        } else {
            self.sequence_number += 1;
            self.epoch = batch_epoch;
        }
    }
}

#[derive(Debug)]
struct DepositedTransaction {
    source_hash: H256,
    from: Address,
    to: Address,
    mint: U256,
    value: U256,
    gas: u64,
    is_system_tx: bool,
    data: Vec<u8>,
}

impl From<AttributesDeposited> for DepositedTransaction {
    fn from(attributes_deposited: AttributesDeposited) -> Self {
        let hash = attributes_deposited.hash.to_fixed_bytes();
        let seq = H256::from_low_u64_be(attributes_deposited.sequence_number).to_fixed_bytes();
        let h = keccak256([hash, seq].concat());

        let domain = H256::from_low_u64_be(1).to_fixed_bytes();
        let source_hash = H256::from_slice(&keccak256([domain, h].concat()));

        let system_accounts = SystemAccounts::default();
        let from = system_accounts.attributes_depositor;
        let to = system_accounts.attributes_predeploy;

        let data = attributes_deposited.encode();

        Self {
            source_hash,
            from,
            to,
            mint: U256::zero(),
            value: U256::zero(),
            gas: 150_000_000,
            is_system_tx: true,
            data,
        }
    }
}

impl From<UserDeposited> for DepositedTransaction {
    fn from(user_deposited: UserDeposited) -> Self {
        let hash = user_deposited.l1_block_hash.to_fixed_bytes();
        let log_index = user_deposited.log_index.into();
        let h = keccak256([hash, log_index].concat());

        let domain = H256::from_low_u64_be(0).to_fixed_bytes();
        let source_hash = H256::from_slice(&keccak256([domain, h].concat()));

        let to = if user_deposited.is_creation {
            Address::zero()
        } else {
            user_deposited.to
        };

        Self {
            source_hash,
            from: user_deposited.from,
            to,
            mint: user_deposited.mint,
            value: user_deposited.value,
            gas: user_deposited.gas,
            is_system_tx: false,
            data: user_deposited.data,
        }
    }
}

impl Encodable for DepositedTransaction {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.append_raw(&[0x7E], 1);
        s.begin_list(8);
        s.append(&self.source_hash);
        s.append(&self.from);
        s.append(&self.to);
        s.append(&self.mint);
        s.append(&self.value);
        s.append(&self.gas);
        s.append(&self.is_system_tx);
        s.append(&self.data);
    }
}

#[derive(Debug)]
struct AttributesDeposited {
    number: u64,
    timestamp: u64,
    base_fee: U256,
    hash: H256,
    sequence_number: u64,
    batcher_hash: H256,
    fee_overhead: U256,
    fee_scalar: U256,
}

impl AttributesDeposited {
    fn from_block(block: &Block<Transaction>, seq: u64, batch_sender: &Address) -> Self {
        let mut batch_sender_bytes = batch_sender.as_bytes().to_vec();
        let mut batcher_hash = iter::repeat(0).take(12).collect::<Vec<_>>();
        batcher_hash.append(&mut batch_sender_bytes);

        Self {
            number: block.number.unwrap().as_u64(),
            timestamp: block.timestamp.as_u64(),
            base_fee: block.base_fee_per_gas.unwrap(),
            hash: block.hash.unwrap(),
            sequence_number: seq,
            batcher_hash: H256::from_slice(&batcher_hash),
            fee_overhead: U256::from(2100),
            fee_scalar: U256::from(1000000),
        }
    }

    fn encode(&self) -> Vec<u8> {
        let tokens = vec![
            Token::Uint(self.number.into()),
            Token::Uint(self.timestamp.into()),
            Token::Uint(self.base_fee),
            Token::FixedBytes(self.hash.as_fixed_bytes().to_vec()),
            Token::Uint(self.sequence_number.into()),
            Token::FixedBytes(self.batcher_hash.as_fixed_bytes().to_vec()),
            Token::Uint(self.fee_overhead),
            Token::Uint(self.fee_scalar),
        ];

        let selector = hex::decode("015d8eb9").unwrap();
        let data = encode(&tokens);

        [selector, data].concat()
    }
}

#[derive(Debug, Clone)]
pub struct UserDeposited {
    pub from: Address,
    pub to: Address,
    pub mint: U256,
    pub value: U256,
    pub gas: u64,
    pub is_creation: bool,
    pub data: Vec<u8>,
    pub l1_block_num: u64,
    pub l1_block_hash: H256,
    pub log_index: U256,
}

impl UserDeposited {
    pub fn from_log(log: Log, l1_block_num: u64, l1_block_hash: H256) -> Result<Self> {
        let opaque_data = decode(&[ParamType::Bytes], &log.data)?[0]
            .clone()
            .into_bytes()
            .unwrap();

        let from = Address::try_from(log.topics[1])?;
        let to = Address::try_from(log.topics[2])?;
        let mint = U256::from_big_endian(&opaque_data[0..32]);
        let value = U256::from_big_endian(&opaque_data[32..64]);
        let gas = u64::from_be_bytes(opaque_data[64..72].try_into()?);
        let is_creation = opaque_data[72] != 0;
        let data = opaque_data[73..].to_vec();

        let log_index = log.log_index.unwrap();

        Ok(Self {
            from,
            to,
            mint,
            value,
            gas,
            is_creation,
            data,
            l1_block_num,
            l1_block_hash,
            log_index,
        })
    }
}
