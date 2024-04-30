//! Contains the [OpStackEnrData] struct.

use alloy_rlp::{Decodable, Encodable, RlpDecodable, RlpEncodable};
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
