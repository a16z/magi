use core::fmt::Debug;
use ethers::abi::parse_abi_str;
use ethers::abi::{decode, encode, ParamType, Token};
use ethers::prelude::BaseContract;
use ethers::types::Bytes;
use ethers::types::{Address, Log, H256, U256};
use ethers::utils::rlp::{DecoderError, Rlp};
use ethers::utils::{keccak256, rlp::Decodable, rlp::Encodable, rlp::RlpStream};

use serde::{de::Error, Deserialize, Deserializer, Serialize, Serializer};

use eyre::Result;

use crate::config::SystemAccounts;
use crate::l1::L1Info;

use super::common::Epoch;

/// A raw transaction
#[derive(Clone, PartialEq, Eq)]
pub struct RawTransaction(pub Vec<u8>);

impl RawTransaction {
    pub fn derive_unsafe_epoch(&self) -> Result<(Epoch, u64)> {
        let rlp = Rlp::new(self.0.as_slice());
        let tx = rlp.as_val::<DepositedTransaction>()?;
        let calldata = Bytes::from(tx.data);
        let attr = AttributesDepositedCall::try_from(calldata)?;
        let epoch = Epoch::from(&attr);

        Ok((epoch, attr.sequence_number))
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

/// Deposited L2 transaction.
#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct DepositedTransaction {
    source_hash: H256,
    from: Address,
    to: Option<Address>,
    mint: U256,
    value: U256,
    gas: u64,
    is_system_tx: bool,
    data: Vec<u8>,
}

impl From<AttributesDeposited> for DepositedTransaction {
    fn from(attributes_deposited: AttributesDeposited) -> Self {
        let hash = attributes_deposited.hash.to_fixed_bytes();
        let seq = H256::from_low_u64_be(attributes_deposited.sequence_number).to_fixed_bytes();
        let h = keccak256([hash, seq].concat());

        let domain = H256::from_low_u64_be(1).to_fixed_bytes();
        let source_hash = H256::from_slice(&keccak256([domain, h].concat()));

        let system_accounts = SystemAccounts::default();
        let from = system_accounts.attributes_depositor;
        let to = Some(system_accounts.attributes_predeploy);

        let data = attributes_deposited.encode();

        Self {
            source_hash,
            from,
            to,
            mint: U256::zero(),
            value: U256::zero(),
            gas: attributes_deposited.gas,
            is_system_tx: attributes_deposited.is_system_tx,
            data,
        }
    }
}

impl From<UserDeposited> for DepositedTransaction {
    fn from(user_deposited: UserDeposited) -> Self {
        let hash = user_deposited.l1_block_hash.to_fixed_bytes();
        let log_index = user_deposited.log_index.into();
        let h = keccak256([hash, log_index].concat());

        let domain = H256::from_low_u64_be(0).to_fixed_bytes();
        let source_hash = H256::from_slice(&keccak256([domain, h].concat()));

        let to = if user_deposited.is_creation {
            None
        } else {
            Some(user_deposited.to)
        };

        Self {
            source_hash,
            from: user_deposited.from,
            to,
            mint: user_deposited.mint,
            value: user_deposited.value,
            gas: user_deposited.gas,
            is_system_tx: false,
            data: user_deposited.data,
        }
    }
}

impl Encodable for DepositedTransaction {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.append_raw(&[0x7E], 1);
        s.begin_list(8);
        s.append(&self.source_hash);
        s.append(&self.from);

        if let Some(to) = self.to {
            s.append(&to);
        } else {
            s.append(&"");
        }

        s.append(&self.mint);
        s.append(&self.value);
        s.append(&self.gas);
        s.append(&self.is_system_tx);
        s.append(&self.data);
    }
}

impl Decodable for DepositedTransaction {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        if !rlp.is_data() {
            return Err(DecoderError::RlpExpectedToBeData);
        }

        if rlp.data().unwrap() != [0x7E] {
            return Err(DecoderError::Custom(
                "rlp data for deposited tx contains wrong prefix",
            ));
        }

        let list_rlp = Rlp::new(&rlp.as_raw()[1..]);

        let source_hash = list_rlp.val_at(0)?;
        let from: Address = list_rlp.val_at(1)?;
        let to = list_rlp.val_at(2).ok();
        let mint = list_rlp.val_at(3)?;
        let value = list_rlp.val_at(4)?;
        let gas = list_rlp.val_at(5)?;
        let is_system_tx = list_rlp.val_at(6)?;
        let data = list_rlp.val_at(7)?;

        Ok(DepositedTransaction {
            source_hash,
            from,
            to,
            mint,
            value,
            gas,
            is_system_tx,
            data,
        })
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AttributesDeposited {
    number: u64,
    timestamp: u64,
    base_fee: U256,
    hash: H256,
    sequence_number: u64,
    batcher_hash: H256,
    fee_overhead: U256,
    fee_scalar: U256,
    gas: u64,
    is_system_tx: bool,
}

impl AttributesDeposited {
    pub fn from_block_info(
        l1_info: &L1Info,
        seq: u64,
        batch_timestamp: u64,
        regolith_time: u64,
    ) -> Self {
        let is_regolith = Self::get_regolith(batch_timestamp, regolith_time);
        let is_system_tx = !is_regolith;
        let gas = Self::get_gas(is_regolith);

        Self {
            number: l1_info.block_info.number,
            timestamp: l1_info.block_info.timestamp,
            base_fee: l1_info.block_info.base_fee,
            hash: l1_info.block_info.hash,
            sequence_number: seq,
            batcher_hash: l1_info.system_config.batcher_hash(),
            fee_overhead: l1_info.system_config.overhead,
            fee_scalar: l1_info.system_config.scalar,
            gas,
            is_system_tx,
        }
    }

    fn get_regolith(timestamp: u64, relogith_time: u64) -> bool {
        timestamp >= relogith_time
    }

    fn get_gas(is_regolith: bool) -> u64 {
        if is_regolith {
            1_000_000
        } else {
            150_000_000
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let tokens = vec![
            Token::Uint(self.number.into()),
            Token::Uint(self.timestamp.into()),
            Token::Uint(self.base_fee),
            Token::FixedBytes(self.hash.as_fixed_bytes().to_vec()),
            Token::Uint(self.sequence_number.into()),
            Token::FixedBytes(self.batcher_hash.as_fixed_bytes().to_vec()),
            Token::Uint(self.fee_overhead),
            Token::Uint(self.fee_scalar),
        ];

        let selector = hex::decode("015d8eb9").unwrap();
        let data = encode(&tokens);

        [selector, data].concat()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AttributesDepositedCall {
    pub number: u64,
    pub timestamp: u64,
    pub basefee: U256,
    pub hash: H256,
    pub sequence_number: u64,
    pub batcher_hash: H256,
    pub fee_overhead: U256,
    pub fee_scalar: U256,
}

type SetL1BlockValueInput = (u64, u64, U256, H256, u64, H256, U256, U256);
const L1_BLOCK_CONTRACT_ABI: &str = r#"[
    function setL1BlockValues(uint64 _number,uint64 _timestamp, uint256 _basefee, bytes32 _hash,uint64 _sequenceNumber,bytes32 _batcherHash,uint256 _l1FeeOverhead,uint256 _l1FeeScalar) external
]"#;

impl TryFrom<Bytes> for AttributesDepositedCall {
    type Error = eyre::Report;

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
    fn from(call: &AttributesDepositedCall) -> Self {
        Self {
            number: call.number,
            timestamp: call.timestamp,
            hash: call.hash,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UserDeposited {
    pub from: Address,
    pub to: Address,
    pub mint: U256,
    pub value: U256,
    pub gas: u64,
    pub is_creation: bool,
    pub data: Vec<u8>,
    pub l1_block_num: u64,
    pub l1_block_hash: H256,
    pub log_index: U256,
}

impl TryFrom<Log> for UserDeposited {
    type Error = eyre::Report;

    fn try_from(log: Log) -> Result<Self, Self::Error> {
        let opaque_data = decode(&[ParamType::Bytes], &log.data)?[0]
            .clone()
            .into_bytes()
            .unwrap();

        let from = Address::from(log.topics[1]);
        let to = Address::from(log.topics[2]);
        let mint = U256::from_big_endian(&opaque_data[0..32]);
        let value = U256::from_big_endian(&opaque_data[32..64]);
        let gas = u64::from_be_bytes(opaque_data[64..72].try_into()?);
        let is_creation = opaque_data[72] != 0;
        let data = opaque_data[73..].to_vec();

        let l1_block_num = log
            .block_number
            .ok_or(eyre::eyre!("block num not found"))?
            .as_u64();

        let l1_block_hash = log.block_hash.ok_or(eyre::eyre!("block hash not found"))?;
        let log_index = log.log_index.unwrap();

        Ok(Self {
            from,
            to,
            mint,
            value,
            gas,
            is_creation,
            data,
            l1_block_num,
            l1_block_hash,
            log_index,
        })
    }
}

#[cfg(test)]
mod tests {

    mod raw_transaction {
        use std::str::FromStr;

        use crate::types::attributes::RawTransaction;
        use ethers::types::H256;

        #[test]
        fn derive_unsafe_epoch() -> eyre::Result<()> {
            let tx = "7ef90159a0ec677ebcdc68441150dad4d485af314aaeb8a06d200e873d0ea1484ac47ce33194deaddeaddeaddeaddeaddeaddeaddeaddead00019442000000000000000000000000000000000000158080830f424080b90104015d8eb9000000000000000000000000000000000000000000000000000000000000005700000000000000000000000000000000000000000000000000000000651f0495000000000000000000000000000000000000000000000000000000000000233579d0a6b649ad11c53645d2115d7912695401b73a35306642cbae97032b31b22b00000000000000000000000000000000000000000000000000000000000000000000000000000000000000003c44cdddb6a900fa2b585dd299e03d12fa4293bc000000000000000000000000000000000000000000000000000000000000083400000000000000000000000000000000000000000000000000000000000f4240";
            let bytes = hex::decode(tx)?;
            let raw_tx = RawTransaction(bytes);

            let expected_hash = H256::from_str(
                "0x79d0a6b649ad11c53645d2115d7912695401b73a35306642cbae97032b31b22b",
            )?;

            let (epoch, seq_num) = raw_tx.derive_unsafe_epoch()?;
            assert!(epoch.number == 87);
            assert!(epoch.timestamp == 1696531605);
            assert!(epoch.hash == expected_hash);
            assert!(seq_num == 0);

            Ok(())
        }
    }

    mod deposited_transaction {
        use rand::Rng;

        use crate::types::attributes::DepositedTransaction;
        use ethers::{
            types::{Address, H256, U256},
            utils::rlp::{Encodable, Rlp},
        };

        #[test]
        fn decodable_no_recipient() -> eyre::Result<()> {
            let mut rng = rand::thread_rng();

            let tx = DepositedTransaction {
                source_hash: H256::random(),
                from: Address::random(),
                to: Some(Address::random()),
                mint: U256::from(rng.gen::<u128>()),
                value: U256::from(rng.gen::<u128>()),
                gas: rng.gen::<u64>(),
                data: rng.gen::<[u8; 32]>().to_vec(),
                is_system_tx: rng.gen_bool(1.0 / 2.0),
            };

            let rpl_bytes = tx.rlp_bytes();
            let rlp = Rlp::new(&rpl_bytes);
            let decoded_tx = rlp.as_val::<DepositedTransaction>()?;

            assert!(tx.source_hash == decoded_tx.source_hash);
            assert!(tx.from == decoded_tx.from);
            assert!(tx.to == decoded_tx.to);
            assert!(tx.mint == decoded_tx.mint);
            assert!(tx.value == decoded_tx.value);
            assert!(tx.gas == decoded_tx.gas);
            assert!(tx.data == decoded_tx.data);
            assert!(tx.is_system_tx == decoded_tx.is_system_tx);

            Ok(())
        }

        #[test]
        fn decodable() -> eyre::Result<()> {
            let mut rng = rand::thread_rng();

            let tx = DepositedTransaction {
                source_hash: H256::random(),
                from: Address::random(),
                to: None,
                mint: U256::from(rng.gen::<u128>()),
                value: U256::from(rng.gen::<u128>()),
                gas: rng.gen::<u64>(),
                data: rng.gen::<[u8; 32]>().to_vec(),
                is_system_tx: rng.gen_bool(1.0 / 2.0),
            };

            let rpl_bytes = tx.rlp_bytes();
            let rlp = Rlp::new(&rpl_bytes);
            let decoded_tx = rlp.as_val::<DepositedTransaction>()?;

            assert!(tx.source_hash == decoded_tx.source_hash);
            assert!(tx.from == decoded_tx.from);
            assert!(tx.to == decoded_tx.to);
            assert!(tx.mint == decoded_tx.mint);
            assert!(tx.value == decoded_tx.value);
            assert!(tx.gas == decoded_tx.gas);
            assert!(tx.data == decoded_tx.data);
            assert!(tx.is_system_tx == decoded_tx.is_system_tx);

            Ok(())
        }
    }

    mod attributed_deposited_call {
        use ethers::types::{Bytes, H256};
        use std::str::FromStr;

        use crate::types::attributes::AttributesDepositedCall;

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
