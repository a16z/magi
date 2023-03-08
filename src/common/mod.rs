use std::fmt::Debug;

use ethers_core::{
    types::H256,
    utils::rlp::{Decodable, DecoderError, Rlp},
};
use figment::value::{Dict, Tag, Value};
use serde::{de::Error, Deserialize, Deserializer, Serialize, Serializer};

/// Selected block header info
#[derive(Debug, Clone, Copy, Eq, PartialEq, Default, Serialize, Deserialize)]
pub struct BlockInfo {
    pub hash: H256,
    pub number: u64,
    pub parent_hash: H256,
    pub timestamp: u64,
}

/// A raw transaction
#[derive(Clone, PartialEq, Eq)]
pub struct RawTransaction(pub Vec<u8>);

/// L1 epoch block
#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Epoch {
    pub number: u64,
    pub hash: H256,
    pub timestamp: u64,
}

impl From<BlockInfo> for Value {
    fn from(value: BlockInfo) -> Value {
        let mut dict = Dict::new();
        dict.insert("hash".to_string(), Value::from(value.hash.as_bytes()));
        dict.insert("number".to_string(), Value::from(value.number));
        dict.insert("timestamp".to_string(), Value::from(value.timestamp));
        dict.insert(
            "parent_hash".to_string(),
            Value::from(value.parent_hash.as_bytes()),
        );
        Value::Dict(Tag::Default, dict)
    }
}

impl From<Epoch> for Value {
    fn from(value: Epoch) -> Self {
        let mut dict = Dict::new();
        dict.insert("hash".to_string(), Value::from(value.hash.as_bytes()));
        dict.insert("number".to_string(), Value::from(value.number));
        dict.insert("timestamp".to_string(), Value::from(value.timestamp));
        Value::Dict(Tag::Default, dict)
    }
}

impl Decodable for RawTransaction {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        let tx_bytes: Vec<u8> = rlp.as_val()?;
        Ok(Self(tx_bytes))
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
