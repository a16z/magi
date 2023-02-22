use std::{cell::RefCell, collections::HashMap, rc::Rc, str::FromStr};

use ethers::{
    abi::{encode, Token},
    types::{Address, Block, Transaction, H256, U256},
    utils::{keccak256, rlp::Encodable},
};

use super::{
    batches::{Batch, Batches, RawTransaction},
    Stage,
};

pub struct Attributes {
    prev_stage: Rc<RefCell<Batches>>,
    blocks: Rc<RefCell<HashMap<H256, Block<Transaction>>>>,
    sequence_number: u64,
}

impl Stage for Attributes {
    type Output = PayloadAttributes;

    fn next(&mut self) -> eyre::Result<Option<Self::Output>> {
        Ok(if let Some(batch) = self.prev_stage.borrow_mut().next()? {
            // TODO: handle seq number
            Some(self.derive_attributes(batch))
        } else {
            None
        })
    }
}

impl Attributes {
    pub fn new(
        prev_stage: Rc<RefCell<Batches>>,
        blocks: Rc<RefCell<HashMap<H256, Block<Transaction>>>>,
    ) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            prev_stage,
            blocks,
            sequence_number: 0,
        }))
    }

    fn derive_attributes(&self, batch: Batch) -> PayloadAttributes {
        let blocks = self.blocks.borrow();
        let base_block = blocks.get(&batch.epoch_hash).unwrap();
        let attributes_deposited =
            AttributesDeposited::from_block(base_block, self.sequence_number);
        let attributes_tx = DepositedTransaction::from(attributes_deposited);
        let attributes_tx = RawTransaction(attributes_tx.rlp_bytes().to_vec());

        let mut transactions = vec![attributes_tx];
        let mut rest = batch.transactions;
        transactions.append(&mut rest);

        PayloadAttributes {
            timestamp: batch.timestamp,
            random: base_block.mix_hash.unwrap(),
            suggested_fee_recipient: Address::default(),
            transactions,
            no_tx_pool: true,
            gas_limit: 30_000_000,
        }
    }
}

#[derive(Debug)]
pub struct PayloadAttributes {
    pub timestamp: u64,
    pub random: H256,
    pub suggested_fee_recipient: Address,
    pub transactions: Vec<RawTransaction>,
    pub no_tx_pool: bool,
    pub gas_limit: u64,
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

        let data = attributes_deposited.encode();

        Self {
            source_hash,
            from: Address::from_str("0xdeaddeaddeaddeaddeaddeaddeaddeaddead0001").unwrap(),
            to: Address::from_str("0x4200000000000000000000000000000000000015").unwrap(),
            mint: U256::zero(),
            value: U256::zero(),
            gas: 150_000_000,
            is_system_tx: true,
            data,
        }
    }
}

impl Encodable for DepositedTransaction {
    fn rlp_append(&self, s: &mut ethers::utils::rlp::RlpStream) {
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
    fn from_block(block: &Block<Transaction>, seq: u64) -> Self {
        Self {
            number: block.number.unwrap().as_u64(),
            timestamp: block.timestamp.as_u64(),
            base_fee: block.base_fee_per_gas.unwrap(),
            hash: block.hash.unwrap(),
            sequence_number: seq,
            batcher_hash: H256::from_str(
                "0x0000000000000000000000007431310e026b69bfc676c0013e12a1a11411eec9",
            )
            .unwrap(),
            fee_overhead: U256::from(2100),
            fee_scalar: U256::from(1000000),
        }
    }

    fn encode(&self) -> Vec<u8> {
        let mut tokens = Vec::new();
        tokens.push(Token::Uint(self.number.into()));
        tokens.push(Token::Uint(self.timestamp.into()));
        tokens.push(Token::Uint(self.base_fee));
        tokens.push(Token::FixedBytes(self.hash.as_fixed_bytes().to_vec()));
        tokens.push(Token::Uint(self.sequence_number.into()));
        tokens.push(Token::FixedBytes(
            self.batcher_hash.as_fixed_bytes().to_vec(),
        ));
        tokens.push(Token::Uint(self.fee_overhead));
        tokens.push(Token::Uint(self.fee_scalar));

        let selector = hex::decode("015d8eb9").unwrap();
        let data = encode(&tokens);

        [selector, data].concat()
    }
}
