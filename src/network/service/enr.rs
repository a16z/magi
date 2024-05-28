//! Contains the [OpStackEnrData] struct.

use alloy_rlp::{Buf, Decodable, Encodable, Error};
use anyhow::Result;

#[derive(Debug, Copy, Default, PartialEq, Clone)]
pub struct OpStackEnrData {
    /// Chain ID
    pub chain_id: u64,
    /// The version. Always set to 0.
    pub version: u64,
}

impl Decodable for OpStackEnrData {
    fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
        buf.advance(1); // Advance past the string rlp type
        let (chain_id, rest) =
            unsigned_varint::decode::u64(buf).map_err(|_| Error::Custom("Invalid chain id"))?;
        let (version, _) =
            unsigned_varint::decode::u64(rest).map_err(|_| Error::Custom("Invalid version"))?;
        Ok(OpStackEnrData { chain_id, version })
    }
}

impl Encodable for OpStackEnrData {
    fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
        out.put_u8(0x87); // RLP string type
        let encoded: &mut [u8; 10] = &mut [0; 10];
        let chain_id = unsigned_varint::encode::u64(self.chain_id, encoded);
        out.put_slice(chain_id);
        let version = unsigned_varint::encode::u64(self.version, encoded);
        out.put_slice(version);
    }
}

impl TryFrom<&[u8]> for OpStackEnrData {
    type Error = anyhow::Error;

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
    use alloy_rlp::BytesMut;

    #[test]
    fn test_decode_encode_raw() {
        let raw = &[0x87, 0x7b, 0x01];
        let decoded = OpStackEnrData::try_from(&raw[..]).unwrap();
        assert_eq!(decoded.chain_id, 123);
        assert_eq!(decoded.version, 1);
        let mut buf = BytesMut::new();
        decoded.encode(&mut buf);
        assert_eq!(&buf[..], raw);
    }

    #[test]
    fn test_empty_round_trip() {
        let data = OpStackEnrData {
            chain_id: 0,
            version: 0,
        };
        let bytes: Vec<u8> = data.into();
        let decoded = OpStackEnrData::try_from(bytes.as_slice()).unwrap();
        assert_eq!(decoded.chain_id, 0);
        assert_eq!(decoded.version, 0);
    }

    #[test]
    fn test_round_trip_large() {
        let data = OpStackEnrData {
            chain_id: u64::MAX / 2,
            version: u64::MAX / 3,
        };
        let bytes: Vec<u8> = data.into();
        let decoded = OpStackEnrData::try_from(bytes.as_slice()).unwrap();
        assert_eq!(decoded.chain_id, data.chain_id);
        assert_eq!(decoded.version, data.version);
    }
}
