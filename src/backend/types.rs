use ethers_core::types::{Transaction, TransactionReceipt};
use ethers_core::types::{H256, U64};
use serde::{Deserialize, Serialize};

/// A constructed block with it's L1 origin.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConstructedBlock {
    /// The parent block hash
    pub parent_hash: BlockHash,
    /// The block hash
    pub hash: Option<BlockHash>,
    /// The block number
    pub number: BlockNumber,
    /// The block timestamp
    pub timestamp: u64,
    /// Transactions
    pub transactions: Vec<Transaction>,
    /// Transaction receipts
    pub receipts: Vec<TransactionReceipt>,
}

impl From<sled::IVec> for ConstructedBlock {
    fn from(bytes: sled::IVec) -> Self {
        serde_json::from_slice(bytes.as_ref()).unwrap()
    }
}

impl From<ConstructedBlock> for sled::IVec {
    fn from(val: ConstructedBlock) -> Self {
        sled::IVec::from(serde_json::to_vec(&val).unwrap())
    }
}

/// A block hash
pub type BlockHash = H256;

/// A block number
pub type BlockNumber = U64;
