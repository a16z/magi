use std::fmt::Debug;

use ethers::{
    abi::{parse_abi_str, FixedBytes},
    prelude::BaseContract,
    types::{Block, Bytes, H256, U256},
    utils::rlp::{Decodable, DecoderError, Rlp},
};
use eyre::Result;
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

type SetL1BlockValueInput = (u64, u64, U256, FixedBytes, u64, FixedBytes, U256, U256);

/// Minimal L1Block ABI from the contract [implementation](https://github.com/ethereum-optimism/optimism/blob/develop/packages/contracts-bedrock/contracts/L2/L1Block.sol).
const L1_BLOCK_CONTRACT_ABI: &str = r#"[
                function setL1BlockValues(uint64 _number,uint64 _timestamp, uint256 _basefee, bytes32 _hash,uint64 _sequenceNumber,bytes32 _batcherHash,uint256 _l1FeeOverhead,uint256 _l1FeeScalar) external
            ]"#;

impl TryFrom<Bytes> for Epoch {
    type Error = eyre::Report;

    /// Performs the conversion from the `setL1BlockValues` calldata to [Epoch].
    fn try_from(tx_calldata: Bytes) -> std::result::Result<Self, Self::Error> {
        let abi = BaseContract::from(parse_abi_str(L1_BLOCK_CONTRACT_ABI)?);

        let (number, timestamp, _, hash, ..): SetL1BlockValueInput =
            abi.decode("setL1BlockValues", tx_calldata)?;

        Ok(Self {
            number,
            timestamp,
            hash: H256::from_slice(&hash),
        })
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

#[cfg(test)]
mod tests {

    mod epoch {
        use std::str::FromStr;

        use ethers::types::{Bytes, H256};

        use crate::common::Epoch;

        #[test]
        fn should_convert_from_the_deposited_transaction_calldata() -> eyre::Result<()> {
            // Arrange
            let calldata = "0x015d8eb900000000000000000000000000000000000000000000000000000000008768240000000000000000000000000000000000000000000000000000000064443450000000000000000000000000000000000000000000000000000000000000000e0444c991c5fe1d7291ff34b3f5c3b44ee861f021396d33ba3255b83df30e357d00000000000000000000000000000000000000000000000000000000000000050000000000000000000000007431310e026b69bfc676c0013e12a1a11411eec9000000000000000000000000000000000000000000000000000000000000083400000000000000000000000000000000000000000000000000000000000f4240";

            let expected_hash =
                H256::from_str("0444c991c5fe1d7291ff34b3f5c3b44ee861f021396d33ba3255b83df30e357d")?;
            let expected_block_number = 8874020;
            let expected_timestamp = 1682191440;

            // Act
            let epoch = Epoch::try_from(Bytes::from_str(calldata)?);

            // Assert
            assert!(epoch.is_ok());
            let epoch = epoch.unwrap();

            assert_eq!(epoch.hash, expected_hash);
            assert_eq!(epoch.number, expected_block_number);
            assert_eq!(epoch.timestamp, expected_timestamp);

            Ok(())
        }
    }
}
