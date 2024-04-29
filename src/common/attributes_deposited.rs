use eyre::Result;
use lazy_static::lazy_static;

use alloy_primitives::{B256, U256, Bytes, keccak256};

/// Represents the attributes deposited transcation call
#[derive(Debug)]
pub struct AttributesDepositedCall {
    /// block number
    pub number: u64,
    /// block timestamp
    pub timestamp: u64,
    /// base fee
    pub basefee: U256,
    /// block hash
    pub hash: B256,
    /// sequence number of the L2 block
    pub sequence_number: u64,
    /// batcher hash (should contain an address)
    pub batcher_hash: B256,
    /// L1 fee overhead
    pub fee_overhead: U256,
    /// L1 fee scalar
    pub fee_scalar: U256,
    /// Blob base fee scalar (after Ecotone)
    pub blob_base_fee_scalar: Option<u32>,
    /// Blob base fee (after Ecotone)
    pub blob_base_fee: Option<U256>,
}

const L1_INFO_BEDROCK_LEN: usize = 4 + 32 * 8;
const L1_INFO_BEDROCK_SIGNATURE: &str =
    "setL1BlockValues(uint64,uint64,uint256,bytes32,uint64,bytes32,uint256,uint256)";

const L1_INFO_ECOTONE_LEN: usize = 4 + 32 * 5;
const L1_INFO_ECOTONE_SIGNATURE: &str = "setL1BlockValuesEcotone()";

lazy_static! {
    static ref SET_L1_BLOCK_VALUES_BEDROCK_SELECTOR: [u8; 4] = keccak256(L1_INFO_BEDROCK_SIGNATURE)
        [..4]
        .try_into()
        .unwrap();
    static ref SET_L1_BLOCK_VALUES_ECOTONE_SELECTOR: [u8; 4] = keccak256(L1_INFO_ECOTONE_SIGNATURE)
        [..4]
        .try_into()
        .unwrap();
}

impl AttributesDepositedCall {
    /// Bedrock Binary Format
    /// ```md
    /// +---------+--------------------------+
    /// | Bytes   | Field                    |
    /// +---------+--------------------------+
    /// | 4       | Function signature       |
    /// | 32      | Number                   |
    /// | 32      | Time                     |
    /// | 32      | BaseFee                  |
    /// | 32      | BlockHash                |
    /// | 32      | SequenceNumber           |
    /// | 32      | BatcherHash              |
    /// | 32      | L1FeeOverhead            |
    /// | 32      | L1FeeScalar              |
    /// +---------+--------------------------+
    /// ```
    pub fn try_from_bedrock(calldata: Bytes) -> Result<Self> {
        let mut cursor = 0;

        if calldata.len() != L1_INFO_BEDROCK_LEN {
            eyre::bail!("invalid calldata length");
        }

        let selector = &calldata[cursor..cursor + 4];
        if selector != *SET_L1_BLOCK_VALUES_BEDROCK_SELECTOR {
            eyre::bail!("invalid selector");
        }
        cursor += 4;

        let number = U256::from_be_slice(calldata[cursor..cursor + 32].try_into()?);
        // down-casting to u64 is safe for the block number
        let number = number.try_into().map_err(|_| eyre::eyre!("invalid block number"))?;
        cursor += 32;

        let timestamp = U256::from_be_slice(calldata[cursor..cursor + 32].try_into()?);
        // down-casting to u64 is safe for UNIX timestamp
        let timestamp = timestamp.try_into().map_err(|_| eyre::eyre!("invalid timestamp"))?;
        cursor += 32;

        let basefee = U256::from_be_slice(&calldata[cursor..cursor + 32]);
        cursor += 32;

        let hash = B256::from_slice(&calldata[cursor..cursor + 32]);
        cursor += 32;

        let seq_num = U256::from_be_slice(calldata[cursor..cursor + 32].try_into()?);
        // down-casting to u64 is safe for the sequence number
        let seq_num = seq_num.try_into().map_err(|_| eyre::eyre!("invalid sequence number"))?;
        cursor += 32;

        let batcher_hash = B256::from_slice(&calldata[cursor..cursor + 32]);
        cursor += 32;

        let fee_overhead = U256::from_be_slice(&calldata[cursor..cursor + 32]);
        cursor += 32;

        let fee_scalar = U256::from_be_slice(&calldata[cursor..cursor + 32]);

        Ok(Self {
            number,
            timestamp,
            basefee,
            hash,
            sequence_number: seq_num,
            batcher_hash,
            fee_overhead,
            fee_scalar,

            // Ecotone fields are not present in Bedrock attributes deposited calls
            blob_base_fee_scalar: None,
            blob_base_fee: None,
        })
    }

    /// Ecotone Binary Format
    /// ```md
    /// +---------+--------------------------+
    /// | Bytes   | Field                    |
    /// +---------+--------------------------+
    /// | 4       | Function signature       |
    /// | 4       | BaseFeeScalar            |
    /// | 4       | BlobBaseFeeScalar        |
    /// | 8       | SequenceNumber           |
    /// | 8       | Timestamp                |
    /// | 8       | L1BlockNumber            |
    /// | 32      | BaseFee                  |
    /// | 32      | BlobBaseFee              |
    /// | 32      | BlockHash                |
    /// | 32      | BatcherHash              |
    /// +---------+--------------------------+
    /// ```
    pub fn try_from_ecotone(calldata: Bytes) -> Result<Self> {
        let mut cursor = 0;

        if calldata.len() != L1_INFO_ECOTONE_LEN {
            eyre::bail!("invalid calldata length");
        }

        let selector = &calldata[cursor..cursor + 4];
        if selector != *SET_L1_BLOCK_VALUES_ECOTONE_SELECTOR {
            eyre::bail!("invalid selector");
        }
        cursor += 4;

        let fee_scalar = u32::from_be_bytes(calldata[cursor..cursor + 4].try_into()?);
        let fee_scalar = U256::from(fee_scalar); // up-casting for backwards compatibility
        cursor += 4;

        let blob_base_fee_scalar =
            Some(u32::from_be_bytes(calldata[cursor..cursor + 4].try_into()?));
        cursor += 4;

        let sequence_number = u64::from_be_bytes(calldata[cursor..cursor + 8].try_into()?);
        cursor += 8;

        let timestamp = u64::from_be_bytes(calldata[cursor..cursor + 8].try_into()?);
        cursor += 8;

        let number = u64::from_be_bytes(calldata[cursor..cursor + 8].try_into()?);
        cursor += 8;

        let basefee = U256::from_be_slice(&calldata[cursor..cursor + 32]);
        cursor += 32;

        let blob_base_fee = Some(U256::from_be_slice(&calldata[cursor..cursor + 32]));
        cursor += 32;

        let hash = B256::from_slice(&calldata[cursor..cursor + 32]);
        cursor += 32;

        let batcher_hash = B256::from_slice(&calldata[cursor..cursor + 32]);

        Ok(Self {
            number,
            timestamp,
            basefee,
            hash,
            sequence_number,
            batcher_hash,
            fee_scalar,
            blob_base_fee,
            blob_base_fee_scalar,

            // The pre-Ecotone L1 fee overhead value is dropped in Ecotone
            fee_overhead: U256::ZERO,
        })
    }
}

#[cfg(test)]
mod tests {
    mod attributed_deposited_call {
        use std::str::FromStr;

        use alloy_primitives::{Bytes, B256, U256};

        use crate::common::AttributesDepositedCall;

        #[test]
        fn decode_from_bytes_bedrock() -> eyre::Result<()> {
            // Arrange
            let calldata = "0x015d8eb900000000000000000000000000000000000000000000000000000000008768240000000000000000000000000000000000000000000000000000000064443450000000000000000000000000000000000000000000000000000000000000000e0444c991c5fe1d7291ff34b3f5c3b44ee861f021396d33ba3255b83df30e357d00000000000000000000000000000000000000000000000000000000000000050000000000000000000000007431310e026b69bfc676c0013e12a1a11411eec9000000000000000000000000000000000000000000000000000000000000083400000000000000000000000000000000000000000000000000000000000f4240";

            let expected_hash =
                B256::from_str("0444c991c5fe1d7291ff34b3f5c3b44ee861f021396d33ba3255b83df30e357d")?;
            let expected_block_number = 8874020;
            let expected_timestamp = 1682191440;

            // Act
            let call = AttributesDepositedCall::try_from_bedrock(Bytes::from_str(calldata)?);

            // Assert
            assert!(call.is_ok());
            let call = call.unwrap();

            assert_eq!(call.hash, expected_hash);
            assert_eq!(call.number, expected_block_number);
            assert_eq!(call.timestamp, expected_timestamp);

            Ok(())
        }

        #[test]
        fn decode_from_bytes_ecotone() -> eyre::Result<()> {
            // Arrange
            // https://goerli-optimism.etherscan.io/tx/0xc2288c5d1f6123406bfe8662bdbc1a3c999394da2e6f444f5aa8df78136f36ba
            let calldata = "0x440a5e2000001db0000d273000000000000000050000000065c8ad6c0000000000a085a20000000000000000000000000000000000000000000000000000000000000008000000000000000000000000000000000000000000000000000000041dfd80f2c8af7d7ba1c1a3962026e5c96b9105d528f8fed65c56cfa731a8751c7f712eb70000000000000000000000007431310e026b69bfc676c0013e12a1a11411eec9";

            let expected_hash = B256::from_str(
                "0xc8af7d7ba1c1a3962026e5c96b9105d528f8fed65c56cfa731a8751c7f712eb7",
            );
            let expected_block_number = 10519970;
            let expected_timestamp = 1707650412;
            let expected_blob_base_fee_scalar = Some(862000);
            let expected_blob_base_fee = Some(U256::from(17683022066u64));

            // Act
            let call = AttributesDepositedCall::try_from_ecotone(Bytes::from_str(calldata)?);

            // Assert
            assert!(call.is_ok());
            let call = call.unwrap();

            assert_eq!(call.hash, expected_hash?);
            assert_eq!(call.number, expected_block_number);
            assert_eq!(call.timestamp, expected_timestamp);
            assert_eq!(call.blob_base_fee_scalar, expected_blob_base_fee_scalar);
            assert_eq!(call.blob_base_fee, expected_blob_base_fee);

            Ok(())
        }
    }
}
