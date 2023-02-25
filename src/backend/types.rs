use ethers_core::types::{Transaction, TransactionReceipt};
use ethers_core::types::{H256, U64};
use serde::{Deserialize, Serialize};

/// A database identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DatabaseId {
    Full((BlockHash, BlockNumber)),
    Number(BlockNumber),
}

impl DatabaseId {
    pub fn as_bytes(&self) -> Vec<u8> {
        match self {
            Self::Full((h, _)) => h.as_bytes().to_vec(),
            Self::Number(n) => n.as_u64().to_le_bytes().to_vec(),
        }
    }
}

impl From<BlockHash> for DatabaseId {
    fn from(hash: BlockHash) -> Self {
        // Note: we don't need the block number if we have the hash, so ignore
        Self::Full((hash, 0.into()))
    }
}

impl From<BlockNumber> for DatabaseId {
    fn from(number: BlockNumber) -> Self {
        Self::Number(number)
    }
}

impl From<&ConstructedBlock> for DatabaseId {
    fn from(block: &ConstructedBlock) -> Self {
        match block.hash {
            Some(h) => Self::Full((h, block.number)),
            None => Self::Number(block.number),
        }
    }
}

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
        let serialized = match serde_json::to_vec(&val) {
            Ok(v) => v,
            Err(e) => {
                panic!("Failed to serialize ConstructedBlock: {}", e)
            }
        };
        sled::IVec::from(serialized)
    }
}

/// A block hash
pub type BlockHash = H256;

/// A block number
pub type BlockNumber = U64;
