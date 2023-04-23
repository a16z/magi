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

impl Epoch {
    pub async fn from_l2_block(l2_block_no: u64, config: &Config) -> Result<Self> {
        let provider = Provider::try_from(&config.l1_rpc_url)?;
        let l2_block_no_hash = H256::from_slice(l2_block_no.to_be_bytes().as_ref());

        let filter = Filter::new()
            .address(config.chain.l2_output_oracle)
            .topic0(ValueOrArray::Value(Some(*OUTPUT_PROPOSED_TOPIC)))
            .topic3(ValueOrArray::Value(Some(l2_block_no_hash)))
            .select(config.chain.l1_start_epoch.number..);

        let l1_batch = provider.get_logs(&filter).await?;

        // let l1_batch = provider
        //     .get_logs(&ethers::types::Filter {
        //         block_option: ethers::types::FilterBlockOption::Range {
        //             from_block: Some(ethers::types::BlockNumber::Number(0.into())),
        //             to_block: Some(ethers::types::BlockNumber::Number(l2_block.number.into())),
        //         },
        //         address: Some(ethers::types::ValueOrArray::Value(
        //             ethers::types::Address::from_str("0x4200000000000000000000000000000000000010")?,
        //         )),
        //         topics: [None, None, None, None],
        //     })
        //     .await?
        //     .into_iter()
        //     .find(|log| {
        //         let rlp = Rlp::new(&log.data.0);
        //         let block_number = rlp.val_at::<u64>(0).unwrap();
        //         block_number == l2_block.number
        //     })
        //     .ok_or(eyre::eyre!("could not find L1 batch for L2 block"))?;

        // let rlp = Rlp::new(&l1_batch.data.0);
        // let block_number = rlp.val_at::<u64>(0).unwrap();
        // let block_hash = rlp.val_at::<ethers::types::H256>(1).unwrap();
        // let timestamp = rlp.val_at::<u64>(2).unwrap();

        // Ok(Self {
        //     number: block_number,
        //     hash: block_hash,
        //     timestamp,
        // })

        todo!()
    }
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
