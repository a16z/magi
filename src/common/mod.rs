use std::fmt::Debug;

use ethers::{
    providers::{Middleware, Provider},
    types::{Block, Filter, ValueOrArray, H256},
    utils::rlp::{Decodable, DecoderError, Rlp},
};
use eyre::Result;
use figment::value::{Dict, Tag, Value};
use serde::{de::Error, Deserialize, Deserializer, Serialize, Serializer};

use crate::{config::Config, l1::OUTPUT_PROPOSED_TOPIC};

/// Selected block header info
#[derive(Debug, Clone, Copy, Eq, PartialEq, Default, Serialize, Deserialize)]
pub struct BlockInfo {
    pub hash: H256,
    pub number: u64,
    pub parent_hash: H256,
    pub timestamp: u64,
}

impl BlockInfo {
    pub async fn from_block_hash(hash: H256, rpc_url: &str) -> Result<Self> {
        let provider = Provider::try_from(rpc_url)?;
        let block = match provider.get_block(hash).await {
            Ok(Some(block)) => block,
            Ok(None) => return Err(eyre::eyre!("could not find block with hash: {hash}")),
            Err(e) => return Err(e.into()),
        };

        Ok(Self {
            hash: block.hash.unwrap(),
            number: block.number.unwrap().as_u64(),
            parent_hash: block.parent_hash,
            timestamp: block.timestamp.as_u64(),
        })
    }
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

impl<T> TryFrom<Block<T>> for BlockInfo {
    type Error = eyre::Report;

    fn try_from(block: Block<T>) -> Result<Self> {
        let number = block
            .number
            .ok_or(eyre::eyre!("block not included"))?
            .as_u64();

        let hash = block.hash.ok_or(eyre::eyre!("block not included"))?;

        Ok(BlockInfo {
            number,
            hash,
            parent_hash: block.parent_hash,
            timestamp: block.timestamp.as_u64(),
        })
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
