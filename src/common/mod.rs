use std::{fmt::Debug, str::FromStr};

use figment::value::{Value, Dict, Tag};
use ethers_core::{
    types::H256,
    utils::rlp::{Decodable, DecoderError, Rlp},
};
use serde::{de::Error, Deserialize, Deserializer, Serialize, Serializer};

/// A Block Identifier
#[derive(Debug, Clone, Copy, Eq, PartialEq, Deserialize)]
pub struct BlockID {
    pub hash: H256,
    pub number: u64,
    pub parent_hash: H256,
}

impl From<BlockID> for Value {
    fn from(value: BlockID) -> Value {
        let mut dict = Dict::new();
        dict.insert("hash".to_string(), Value::from(value.hash.as_bytes()));
        dict.insert("number".to_string(), Value::from(value.number));
        dict.insert("parent_hash".to_string(), Value::from(value.parent_hash.as_bytes()));
        Value::Dict(Tag::Default, dict)
    }
}

impl FromStr for BlockID {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(hash) = H256::from_str(s) {
            return Ok(Self {
                hash,
                number: 0,
                parent_hash: H256::zero(),
            });
        }
        if let Ok(number) = u64::from_str(s) {
            return Ok(Self {
                hash: H256::zero(),
                number,
                parent_hash: H256::zero(),
            });
        }
        // Otherwise, use 0 as the block number
        Ok(Self {
            hash: H256::zero(),
            number: 0,
            parent_hash: H256::zero(),
        })
    }
}

/// A raw transaction
#[derive(Clone, PartialEq, Eq)]
pub struct RawTransaction(pub Vec<u8>);

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
