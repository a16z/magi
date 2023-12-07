use super::config::SystemAccounts;
use crate::common::{Epoch, RawTransaction};
use ethers::{
    abi::parse_abi_str,
    contract::Lazy,
    prelude::BaseContract,
    types::{Bytes, Selector, Transaction, H256, U256},
    utils::rlp::{Decodable, Rlp},
};
use std::str::FromStr;

pub type AppendTxBatchInput = Bytes;
pub const APPEND_TX_BATCH_ABI_STR: &str = r#"[
    function appendTxBatch(bytes calldata txBatchData) external
]"#;
pub static APPEND_TX_BATCH_ABI: Lazy<BaseContract> = Lazy::new(|| {
    BaseContract::from(parse_abi_str(APPEND_TX_BATCH_ABI_STR).expect("abi must be valid"))
});
pub static APPEND_TX_BATCH_SELECTOR: Lazy<Selector> = Lazy::new(|| {
    APPEND_TX_BATCH_ABI
        .abi()
        .function("appendTxBatch")
        .expect("function must be present")
        .short_signature()
});

pub type SetL1OracleValuesInput = (U256, U256, U256, H256, H256);
pub const SET_L1_ORACLE_VALUES_ABI_STR: &str = r#"[
    function setL1OracleValues(uint256 _number,uint256 _timestamp,uint256 _baseFee,bytes32 _hash,bytes32 _stateRoot) external
]"#;
pub static SET_L1_ORACLE_VALUES_ABI: Lazy<BaseContract> = Lazy::new(|| {
    BaseContract::from(parse_abi_str(SET_L1_ORACLE_VALUES_ABI_STR).expect("abi must be valid"))
});
pub static SET_L1_ORACLE_VALUES_SELECTOR: Lazy<Selector> = Lazy::new(|| {
    SET_L1_ORACLE_VALUES_ABI
        .abi()
        .function("setL1OracleValues")
        .expect("function must be present")
        .short_signature()
});

pub const L1_ORACLE_NUMBER_LOC_STR: &str =
    "0x00000000000000000000000000000000000000000000000000000000000000fb";
pub const L1_ORACLE_TIMESTAMP_LOC_STR: &str =
    "0x00000000000000000000000000000000000000000000000000000000000000fc";
pub const L1_ORACLE_HASH_LOC_STR: &str =
    "0x00000000000000000000000000000000000000000000000000000000000000fe";
pub static L1_ORACLE_NUMBER_LOC: Lazy<H256> =
    Lazy::new(|| H256::from_str(L1_ORACLE_NUMBER_LOC_STR).unwrap());
pub static L1_ORACLE_TIMESTAMP_LOC: Lazy<H256> =
    Lazy::new(|| H256::from_str(L1_ORACLE_TIMESTAMP_LOC_STR).unwrap());
pub static L1_ORACLE_HASH_LOC: Lazy<H256> =
    Lazy::new(|| H256::from_str(L1_ORACLE_HASH_LOC_STR).unwrap());

impl From<&SetL1OracleValuesInput> for Epoch {
    fn from(input: &SetL1OracleValuesInput) -> Self {
        Self {
            number: input.0.as_u64(),
            timestamp: input.1.as_u64(),
            hash: input.3,
        }
    }
}

/// Try to decode the input of a transaction as an `setL1OracleValues`.
/// Requires the transaction to be sent to the L1 oracle.
pub fn try_decode_l1_oracle_values(tx: &Transaction) -> Option<SetL1OracleValuesInput> {
    if tx.to? != SystemAccounts::default().l1_oracle {
        return None;
    }

    SET_L1_ORACLE_VALUES_ABI
        .decode_with_selector(*SET_L1_ORACLE_VALUES_SELECTOR, &tx.input.0)
        .ok()
}

impl TryFrom<&RawTransaction> for SetL1OracleValuesInput {
    type Error = eyre::Report;

    fn try_from(tx: &RawTransaction) -> Result<Self, Self::Error> {
        let tx = Transaction::decode(&Rlp::new(&tx.0))?;
        try_decode_l1_oracle_values(&tx).ok_or(eyre::eyre!("could not decode oracle values"))
    }
}
