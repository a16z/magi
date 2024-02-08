use std::fmt::Debug;

use ethers::{
    abi::parse_abi_str,
    prelude::BaseContract,
    types::{Block, Bytes, Transaction, H256, U256},
    utils::rlp::{Decodable, DecoderError, Rlp},
};
use eyre::Result;
use figment::value::{Dict, Tag, Value};
use serde::{de::Error, Deserialize, Deserializer, Serialize, Serializer};

use crate::engine::ExecutionPayload;

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

impl TryFrom<Block<Transaction>> for BlockInfo {
    type Error = eyre::Report;

    /// Converts a [Block] to [BlockInfo]
    fn try_from(block: Block<Transaction>) -> Result<Self> {
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

impl From<&ExecutionPayload> for BlockInfo {
    /// Converts an [ExecutionPayload] to [BlockInfo]
    fn from(value: &ExecutionPayload) -> Self {
        Self {
            number: value.block_number.as_u64(),
            hash: value.block_hash,
            parent_hash: value.parent_hash,
            timestamp: value.timestamp.as_u64(),
        }
    }
}

/// Represents the `setL1BlockValues` transaction inputs included in the first transaction of every L2 block.
pub struct AttributesDepositedCall {
    /// The L1 block number of the corresponding epoch this belongs to.
    pub number: u64,
    /// The L1 block timestamp of the corresponding epoch this belongs to.
    pub timestamp: u64,
    /// The L1 block basefee of the corresponding epoch this belongs to.
    pub basefee: U256,
    /// The L1 block hash of the corresponding epoch this belongs to.
    pub hash: H256,
    /// The L2 block's position within the epoch.
    pub sequence_number: u64,
    /// A versioned hash of the current authorized batcher sender.
    pub batcher_hash: H256,
    /// The current L1 fee overhead to apply to L2 transactions cost computation. Unused after Ecotone hard fork.
    pub fee_overhead: U256,
    /// The current L1 fee scalar to apply to L2 transactions cost computation. Unused after Ecotone hard fork.
    pub fee_scalar: U256,
}

type SetL1BlockValueInput = (u64, u64, U256, H256, u64, H256, U256, U256);
const L1_BLOCK_CONTRACT_ABI: &str = r#"[
    function setL1BlockValues(uint64 _number,uint64 _timestamp, uint256 _basefee, bytes32 _hash,uint64 _sequenceNumber,bytes32 _batcherHash,uint256 _l1FeeOverhead,uint256 _l1FeeScalar) external
]"#;

impl TryFrom<Bytes> for AttributesDepositedCall {
    type Error = eyre::Report;

    /// Decodes and converts the given bytes (calldata) into [AttributesDepositedCall].
    fn try_from(value: Bytes) -> Result<Self> {
        let abi = BaseContract::from(parse_abi_str(L1_BLOCK_CONTRACT_ABI)?);

        let (
            number,
            timestamp,
            basefee,
            hash,
            sequence_number,
            batcher_hash,
            fee_overhead,
            fee_scalar,
        ): SetL1BlockValueInput = abi.decode("setL1BlockValues", value)?;

        Ok(Self {
            number,
            timestamp,
            basefee,
            hash,
            sequence_number,
            batcher_hash,
            fee_overhead,
            fee_scalar,
        })
    }
}

impl From<&AttributesDepositedCall> for Epoch {
    /// Converts [AttributesDepositedCall] to an [Epoch] consisting of the number, hash & timestamp of the corresponding L1 epoch block.
    fn from(call: &AttributesDepositedCall) -> Self {
        Self {
            number: call.number,
            timestamp: call.timestamp,
            hash: call.hash,
        }
    }
}

impl Decodable for RawTransaction {
    /// Decodes RLP encoded bytes into [RawTransaction] bytes
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

#[cfg(test)]
mod tests {
    mod attributed_deposited_call {
        use std::str::FromStr;

        use ethers::types::{Bytes, H256};

        use crate::common::AttributesDepositedCall;

        #[test]
        fn decode_from_bytes() -> eyre::Result<()> {
            // Arrange
            let calldata = "0x015d8eb900000000000000000000000000000000000000000000000000000000008768240000000000000000000000000000000000000000000000000000000064443450000000000000000000000000000000000000000000000000000000000000000e0444c991c5fe1d7291ff34b3f5c3b44ee861f021396d33ba3255b83df30e357d00000000000000000000000000000000000000000000000000000000000000050000000000000000000000007431310e026b69bfc676c0013e12a1a11411eec9000000000000000000000000000000000000000000000000000000000000083400000000000000000000000000000000000000000000000000000000000f4240";

            let expected_hash =
                H256::from_str("0444c991c5fe1d7291ff34b3f5c3b44ee861f021396d33ba3255b83df30e357d")?;
            let expected_block_number = 8874020;
            let expected_timestamp = 1682191440;

            // Act
            let call = AttributesDepositedCall::try_from(Bytes::from_str(calldata)?);

            // Assert
            assert!(call.is_ok());
            let call = call.unwrap();

            assert_eq!(call.hash, expected_hash);
            assert_eq!(call.number, expected_block_number);
            assert_eq!(call.timestamp, expected_timestamp);

            Ok(())
        }
    }
}
