//! Module containing common types and functions used throughout the crate.

use std::fmt::Debug;

use alloy_primitives::B256;
use alloy_rpc_types::Block;
use eyre::Result;
use figment::value::{Dict, Tag, Value};
use serde::{de::Error, Deserialize, Deserializer, Serialize, Serializer};

use crate::engine::ExecutionPayload;

/// Attributes deposited transaction helpers
pub mod attributes_deposited;
pub use attributes_deposited::AttributesDepositedCall;

/// Selected block header info
#[derive(Debug, Clone, Copy, Eq, PartialEq, Default, Serialize, Deserialize)]
pub struct BlockInfo {
    /// The block hash
    pub hash: B256,
    /// The block number
    pub number: u64,
    /// The parent block hash
    pub parent_hash: B256,
    /// The block timestamp
    pub timestamp: u64,
}

/// A raw transaction
#[derive(Clone, alloy_rlp::RlpDecodable, alloy_rlp::RlpEncodable, PartialEq, Eq)]
pub struct RawTransaction(pub Vec<u8>);

/// L1 epoch block
#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Epoch {
    /// The block number
    pub number: u64,
    /// The block hash
    pub hash: B256,
    /// The block timestamp
    pub timestamp: u64,
}

impl From<BlockInfo> for Value {
    fn from(value: BlockInfo) -> Value {
        let mut dict = Dict::new();
        dict.insert("hash".to_string(), Value::from(value.hash.as_slice()));
        dict.insert("number".to_string(), Value::from(value.number));
        dict.insert("timestamp".to_string(), Value::from(value.timestamp));
        dict.insert(
            "parent_hash".to_string(),
            Value::from(value.parent_hash.as_slice()),
        );
        Value::Dict(Tag::Default, dict)
    }
}

impl TryFrom<Block> for BlockInfo {
    type Error = eyre::Report;

    /// Converts a [Block] to [BlockInfo]
    fn try_from(block: Block) -> Result<Self> {
        let number = block
            .header
            .number
            .ok_or(eyre::eyre!("block not included"))?
            .try_into()?;

        let hash = block.header.hash.ok_or(eyre::eyre!("block not included"))?;
        let timestamp = block.header.timestamp.try_into()?;

        Ok(BlockInfo {
            number,
            hash,
            parent_hash: block.header.parent_hash,
            timestamp,
        })
    }
}

impl From<Epoch> for Value {
    fn from(value: Epoch) -> Self {
        let mut dict = Dict::new();
        dict.insert("hash".to_string(), Value::from(value.hash.as_slice()));
        dict.insert("number".to_string(), Value::from(value.number));
        dict.insert("timestamp".to_string(), Value::from(value.timestamp));
        Value::Dict(Tag::Default, dict)
    }
}

impl From<&ExecutionPayload> for BlockInfo {
    /// Converts an [ExecutionPayload] to [BlockInfo]
    fn from(value: &ExecutionPayload) -> Self {
        Self {
            number: value.block_number.try_into().unwrap_or_default(),
            hash: value.block_hash,
            parent_hash: value.parent_hash,
            timestamp: value.timestamp.try_into().unwrap_or_default(),
        }
    }
}

impl From<&AttributesDepositedCall> for Epoch {
    /// Converts [AttributesDepositedCall] to an [Epoch] consisting of the number, hash & timestamp of the corresponding L1 epoch block.
    fn from(call: &AttributesDepositedCall) -> Self {
        Self {
            number: call.number,
            timestamp: call.timestamp,
            hash: B256::from_slice(call.hash.as_slice()),
        }
    }
}

impl Debug for RawTransaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x{}", hex::encode(&self.0))
    }
}

impl Serialize for RawTransaction {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format!("0x{}", hex::encode(&self.0)))
    }
}

impl<'de> Deserialize<'de> for RawTransaction {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let tx: String = serde::Deserialize::deserialize(deserializer)?;
        let tx = tx.strip_prefix("0x").unwrap_or(&tx);
        Ok(RawTransaction(hex::decode(tx).map_err(D::Error::custom)?))
    }
}
