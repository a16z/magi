use std::sync::{Arc, RwLock};

use ethers::abi::{decode, encode, ParamType, Token};
use ethers::types::{Address, Log, H256, U256};
use ethers::utils::{keccak256, rlp::Encodable, rlp::RlpStream};

use eyre::Result;

use crate::common::{Epoch, RawTransaction};
use crate::config::{Config, SystemAccounts};
use crate::derive::state::State;
use crate::derive::{get_ecotone_upgrade_transactions, PurgeableIterator};
use crate::engine::PayloadAttributes;
use crate::l1::L1Info;

use super::block_input::BlockInput;

/// Represents the `Payload Attributes Derivation` stage.
pub struct Attributes {
    /// An iterator over [BlockInput]: used to derive [PayloadAttributes]
    block_input_iter: Box<dyn PurgeableIterator<Item = BlockInput<u64>>>,
    /// The current derivation [State]. Contains cached L1 & L2 blocks and details of the current safe head & safe epoch.
    state: Arc<RwLock<State>>,
    /// The sequence number of the block being processed
    sequence_number: u64,
    /// The block hash of the corresponding L1 epoch block.
    epoch_hash: H256,
    /// The global Magi [Config]
    config: Arc<Config>,
}

impl Iterator for Attributes {
    type Item = PayloadAttributes;

    /// Iterates over the next [BlockInput] and returns the [PayLoadAttributes](struct@PayloadAttributes) from this block.
    fn next(&mut self) -> Option<Self::Item> {
        self.block_input_iter
            .next()
            .map(|input| input.with_full_epoch(&self.state).unwrap())
            .map(|batch| self.derive_attributes(batch))
    }
}

impl PurgeableIterator for Attributes {
    /// Purges the [BlockInput] iterator, and sets the [epoch_hash](Attributes::epoch_hash) to the [safe_epoch](State::safe_epoch) hash.
    fn purge(&mut self) {
        self.block_input_iter.purge();
        self.sequence_number = 0;
        self.epoch_hash =
            ethers::types::H256::from_slice(self.state.read().unwrap().safe_epoch.hash.as_slice());
    }
}

impl Attributes {
    /// Creates new [Attributes] and sets the `epoch_hash` to the current L1 safe epoch block hash.
    pub fn new(
        block_input_iter: Box<dyn PurgeableIterator<Item = BlockInput<u64>>>,
        state: Arc<RwLock<State>>,
        config: Arc<Config>,
        seq: u64,
    ) -> Self {
        let epoch_hash = state.read().unwrap().safe_epoch.hash;

        Self {
            block_input_iter,
            state,
            sequence_number: seq,
            epoch_hash: H256::from_slice(epoch_hash.as_slice()),
            config,
        }
    }

    /// Processes a given [BlockInput] and returns [PayloadAttributes] for the block.
    ///
    /// Calls `derive_transactions` to generate the raw transactions
    fn derive_attributes(&mut self, input: BlockInput<Epoch>) -> PayloadAttributes {
        tracing::debug!("deriving attributes from block: {}", input.epoch.number);
        tracing::debug!("batch epoch hash: {:?}", input.epoch.hash);

        let epoch_hash = H256::from_slice(input.epoch.hash.as_slice());
        self.update_sequence_number(epoch_hash);

        let state = self.state.read().unwrap();
        let l1_info = state.l1_info_by_hash(epoch_hash).unwrap();

        let withdrawals = if input.timestamp >= self.config.chain.canyon_time {
            Some(Vec::new())
        } else {
            None
        };

        let l1_inclusion_block = Some(input.l1_inclusion_block);
        let seq_number = Some(self.sequence_number);
        let prev_randao = l1_info.block_info.mix_hash;
        let epoch = Some(input.epoch);
        let transactions = Some(self.derive_transactions(input, l1_info));
        let suggested_fee_recipient = SystemAccounts::default().fee_vault;

        PayloadAttributes {
            timestamp: alloy_primitives::U64::from(input.timestamp),
            prev_randao,
            suggested_fee_recipient,
            transactions,
            no_tx_pool: true,
            gas_limit: alloy_primitives::U64::from(l1_info.system_config.gas_limit),
            withdrawals,
            epoch,
            l1_inclusion_block,
            seq_number,
        }
    }

    /// Derives the deposited transactions and all other L2 user transactions from a given block. Deposited txs include:
    /// - L1 Attributes Deposited (exists as the first tx in every block)
    /// - User deposits sent to the L1 deposit contract (0 or more and will only exist in the first block of the epoch)
    ///
    /// Returns a [RawTransaction] vector containing all of the transactions for the L2 block.
    fn derive_transactions(
        &self,
        input: BlockInput<Epoch>,
        l1_info: &L1Info,
    ) -> Vec<RawTransaction> {
        let mut transactions = Vec::new();

        // L1 info (attributes deposited) transaction, present in every block
        let attributes_tx = self.derive_attributes_deposited(l1_info, input.timestamp);
        transactions.push(attributes_tx);

        // User deposit transactions, present in the first block of every epoch
        if self.sequence_number == 0 {
            let mut user_deposited_txs = self.derive_user_deposited();
            transactions.append(&mut user_deposited_txs);
        }

        // Ecotone upgrade transactions
        if self
            .config
            .chain
            .is_ecotone_activation_block(input.timestamp)
        {
            tracing::info!("found Ecotone activation block; Upgrade transactions added");
            let mut ecotone_upgrade_txs = get_ecotone_upgrade_transactions();
            transactions.append(&mut ecotone_upgrade_txs);
        }

        // Remaining transactions
        let mut rest = input.transactions;
        transactions.append(&mut rest);

        transactions
    }

    /// Derives the attributes deposited transaction for a given block and converts this to a [RawTransaction].
    fn derive_attributes_deposited(
        &self,
        l1_info: &L1Info,
        batch_timestamp: u64,
    ) -> RawTransaction {
        let seq = self.sequence_number;
        let attributes_deposited =
            AttributesDeposited::from_block_info(l1_info, seq, batch_timestamp, &self.config);
        let attributes_tx = DepositedTransaction::from(attributes_deposited);
        RawTransaction(attributes_tx.rlp_bytes().to_vec())
    }

    /// Derives the user deposited txs for the current epoch, and returns a [RawTransaction] vector
    fn derive_user_deposited(&self) -> Vec<RawTransaction> {
        let state = self.state.read().unwrap();
        state
            .l1_info_by_hash(self.epoch_hash)
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

    /// Sets the current sequence number. If `self.epoch_hash` != `batch_epoch_hash` this is set to 0; otherwise it increments by 1.
    ///
    /// Also sets `self.epoch_hash` to `batch_epoch_hash`
    fn update_sequence_number(&mut self, batch_epoch_hash: H256) {
        if self.epoch_hash != batch_epoch_hash {
            self.sequence_number = 0;
        } else {
            self.sequence_number += 1;
        }

        self.epoch_hash = batch_epoch_hash;
    }
}

/// Represents a deposited transaction
#[derive(Debug)]
struct DepositedTransaction {
    /// Unique identifier to identify the origin of the deposit
    source_hash: H256,
    /// Address of the sender
    from: Address,
    /// Address of the recipient, or None if the transaction is a contract creation
    to: Option<Address>,
    /// ETH value to mint on L2
    mint: U256,
    /// ETH value to send to the recipient
    value: U256,
    /// Gas limit for the L2 transaction
    gas: u64,
    /// If true, does not use L2 gas. Always False post-Regolith.
    is_system_tx: bool,
    /// Any additional calldata or contract creation code.
    data: Vec<u8>,
}

impl From<AttributesDeposited> for DepositedTransaction {
    /// Converts [AttributesDeposited] to a [DepositedTransaction]
    fn from(attributes_deposited: AttributesDeposited) -> Self {
        let hash = attributes_deposited.hash.to_fixed_bytes();
        let seq = H256::from_low_u64_be(attributes_deposited.sequence_number).to_fixed_bytes();
        let h = keccak256([hash, seq].concat());

        let domain = H256::from_low_u64_be(1).to_fixed_bytes();
        let source_hash = H256::from_slice(&keccak256([domain, h].concat()));

        let system_accounts = SystemAccounts::default();
        let from = system_accounts.attributes_depositor;
        let to = Some(system_accounts.attributes_predeploy);

        let data = attributes_deposited.encode();

        Self {
            source_hash,
            from: Address::from_slice(from.as_slice()),
            to: to.map(|t| Address::from_slice(t.as_slice())),
            mint: U256::zero(),
            value: U256::zero(),
            gas: attributes_deposited.gas,
            is_system_tx: attributes_deposited.is_system_tx,
            data,
        }
    }
}

impl From<UserDeposited> for DepositedTransaction {
    /// Converts [UserDeposited] to a [DepositedTransaction]
    fn from(user_deposited: UserDeposited) -> Self {
        let hash = user_deposited.l1_block_hash.to_fixed_bytes();
        let log_index = user_deposited.log_index.into();
        let h = keccak256([hash, log_index].concat());

        let domain = H256::from_low_u64_be(0).to_fixed_bytes();
        let source_hash = H256::from_slice(&keccak256([domain, h].concat()));

        let to = if user_deposited.is_creation {
            None
        } else {
            Some(user_deposited.to)
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
    /// Converts a [DepositedTransaction] to RLP bytes and appends to the stream.
    fn rlp_append(&self, s: &mut RlpStream) {
        s.append_raw(&[0x7E], 1);
        s.begin_list(8);
        s.append(&self.source_hash);
        s.append(&self.from);

        if let Some(to) = self.to {
            s.append(&to);
        } else {
            s.append(&"");
        }

        s.append(&self.mint);
        s.append(&self.value);
        s.append(&self.gas);
        s.append(&self.is_system_tx);
        s.append(&self.data);
    }
}

/// Represents the attributes provided as calldata in an attributes deposited transaction.
#[derive(Debug)]
struct AttributesDeposited {
    /// The L1 epoch block number
    number: u64,
    /// The L1 epoch block timestamp
    timestamp: u64,
    /// The L1 epoch base fee
    base_fee: U256,
    /// The L1 epoch block hash
    hash: H256,
    /// The L2 block's position in the epoch
    sequence_number: u64,
    /// A versioned hash of the current authorized batcher sender.
    batcher_hash: H256,
    /// The current L1 fee overhead to apply to L2 transactions cost computation. Unused after Ecotone hard fork.
    fee_overhead: U256,
    /// The current L1 fee scalar to apply to L2 transactions cost computation. Unused after Ecotone hard fork.
    fee_scalar: U256,
    /// Gas limit: 1_000_000 if post-Regolith, otherwise 150_000_000
    gas: u64,
    /// False if post-Regolith, otherwise true
    is_system_tx: bool,
}

impl AttributesDeposited {
    /// Creates [AttributesDeposited] from the given data.
    fn from_block_info(l1_info: &L1Info, seq: u64, batch_timestamp: u64, config: &Config) -> Self {
        let is_regolith = batch_timestamp >= config.chain.regolith_time;
        let is_system_tx = !is_regolith;

        let gas = if is_regolith { 1_000_000 } else { 150_000_000 };

        let batcher_hash = H256::from_slice(l1_info.system_config.batcher_hash().as_slice());
        let fee_overhead = U256::from(l1_info.system_config.l1_fee_overhead.to_be_bytes());
        let fee_scalar = U256::from(l1_info.system_config.l1_fee_scalar.to_be_bytes());
        Self {
            number: l1_info.block_info.number,
            timestamp: l1_info.block_info.timestamp,
            base_fee: U256::from_big_endian(&l1_info.block_info.base_fee.to_be_bytes::<32>()),
            hash: H256::from_slice(l1_info.block_info.hash.as_slice()),
            sequence_number: seq,
            batcher_hash,
            fee_overhead,
            fee_scalar,
            gas,
            is_system_tx,
        }
    }

    /// Encodes [AttributesDeposited] into `setL1BlockValues` transaction calldata, including the selector.
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

/// Represents a user deposited transaction.
#[derive(Debug, Clone)]
pub struct UserDeposited {
    /// Address of the sender
    pub from: Address,
    /// Address of the recipient, or None if the transaction is a contract creation
    pub to: Address,
    /// ETH value to mint on L2
    pub mint: U256,
    /// ETH value to send to the recipient
    pub value: U256,
    /// Gas limit for the L2 transaction
    pub gas: u64,
    /// If this is a contract creation
    pub is_creation: bool,
    /// Calldata or contract creation code if `is_creation` is true.
    pub data: Vec<u8>,
    /// The L1 block number this was submitted in.
    pub l1_block_num: u64,
    /// The L1 block hash this was submitted in.
    pub l1_block_hash: H256,
    /// The index of the emitted deposit event log in the L1 block.
    pub log_index: U256,
}

impl UserDeposited {
    /// Creates a new [UserDeposited] from the given data.
    pub fn new(
        log: alloy_rpc_types::Log,
        l1_block_num: u64,
        l1_block_hash: alloy_primitives::B256,
        log_index: alloy_primitives::U256,
    ) -> Result<Self> {
        let opaque_data = decode(&[ParamType::Bytes], &log.data().data)?[0]
            .clone()
            .into_bytes()
            .ok_or(eyre::eyre!("invalid data"))?;

        let from = Address::from_slice(log.topics()[1].as_slice());
        let to = Address::from_slice(log.topics()[2].as_slice());
        let mint = U256::from_big_endian(&opaque_data[0..32]);
        let value = U256::from_big_endian(&opaque_data[32..64]);
        let gas = u64::from_be_bytes(opaque_data[64..72].try_into()?);
        let is_creation = opaque_data[72] != 0;
        let data = opaque_data[73..].to_vec();

        Ok(Self {
            from,
            to,
            mint,
            value,
            gas,
            is_creation,
            data,
            l1_block_num,
            l1_block_hash: H256::from_slice(l1_block_hash.as_slice()),
            log_index: U256::from_big_endian(&log_index.to_be_bytes::<32>()),
        })
    }
}

impl TryFrom<Log> for UserDeposited {
    type Error = eyre::Report;

    /// Converts the emitted L1 deposit event log into [UserDeposited]
    fn try_from(log: Log) -> Result<Self, Self::Error> {
        let opaque_data = decode(&[ParamType::Bytes], &log.data)?[0]
            .clone()
            .into_bytes()
            .unwrap();

        let from = Address::from(log.topics[1]);
        let to = Address::from(log.topics[2]);
        let mint = U256::from_big_endian(&opaque_data[0..32]);
        let value = U256::from_big_endian(&opaque_data[32..64]);
        let gas = u64::from_be_bytes(opaque_data[64..72].try_into()?);
        let is_creation = opaque_data[72] != 0;
        let data = opaque_data[73..].to_vec();

        let l1_block_num = log
            .block_number
            .ok_or(eyre::eyre!("block num not found"))?
            .as_u64();

        let l1_block_hash = log.block_hash.ok_or(eyre::eyre!("block hash not found"))?;
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
