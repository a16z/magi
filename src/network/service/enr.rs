//! Contains the [OpStackEnrData] struct.

use alloy_rlp::{Decodable, Encodable, RlpEncodable, RlpDecodable};
use eyre::Result;

#[derive(Debug, RlpEncodable, RlpDecodable, PartialEq, Clone)]
pub struct OpStackEnrData {
    /// Chain ID
    pub chain_id: u64,
    /// The version. Always set to 0.
    pub version: u64,
}

impl TryFrom<&[u8]> for OpStackEnrData {
    type Error = eyre::Report;

    /// Converts a slice of RLP encoded bytes to [OpStackEnrData]
    fn try_from(mut value: &[u8]) -> Result<Self> {
        Ok(OpStackEnrData::decode(&mut value)?)
    }
}

impl From<OpStackEnrData> for Vec<u8> {
    /// Converts [OpStackEnrData] to a vector of bytes.
    fn from(value: OpStackEnrData) -> Vec<u8> {
        let mut bytes = Vec::new();
        value.encode(&mut bytes);
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_round_trip_opstack() {
        let data = OpStackEnrData {
            chain_id: 123,
            version: 1,
        };
        let bytes: Vec<u8> = data.into();
        let decoded = OpStackEnrData::try_from(bytes.as_slice()).unwrap();
        assert_eq!(decoded.chain_id, 123);
        assert_eq!(decoded.version, 1);
    }
}
