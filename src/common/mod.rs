use ethers_core::types::H256;

/// A Block Identifier
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct BlockID {
    pub hash: H256,
    pub number: u64,
    pub parent_hash: H256,
}
