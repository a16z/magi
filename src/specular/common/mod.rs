use ethers::{
    abi::parse_abi_str,
    contract::Lazy,
    prelude::BaseContract,
    types::{Bytes, Selector, H256, U256},
};

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
