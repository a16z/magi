use ethers::types::{Block, Transaction, H256};
use figment::value::{Dict, Tag, Value};
use serde::{Deserialize, Serialize};

use crate::engine::ExecutionPayload;

use eyre::Result;

use super::attributes::AttributesDepositedCall;

/// Selected block header info
#[derive(Debug, Clone, Copy, Eq, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockInfo {
    pub hash: H256,
    pub number: u64,
    pub parent_hash: H256,
    pub timestamp: u64,
}

impl<TX> TryFrom<Block<TX>> for BlockInfo {
    type Error = eyre::Report;

    fn try_from(block: Block<TX>) -> Result<Self> {
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

impl From<&ExecutionPayload> for BlockInfo {
    fn from(value: &ExecutionPayload) -> Self {
        Self {
            number: value.block_number.as_u64(),
            hash: value.block_hash,
            parent_hash: value.parent_hash,
            timestamp: value.timestamp.as_u64(),
        }
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

/// L2 block info, referenced to L1 epoch and sequence number.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeadInfo {
    /// The L2 head.
    #[serde(flatten)]
    pub head: BlockInfo,
    /// Referenced L1 epoch.
    #[serde(rename = "l1origin")]
    pub epoch: Epoch,
    /// Sequencer number in the epoch.
    #[serde(rename = "sequenceNumber")]
    pub seq_number: u64,
}

impl HeadInfo {
    pub fn new(head: BlockInfo, epoch: Epoch, seq_number: u64) -> Self {
        Self {
            head,
            epoch,
            seq_number,
        }
    }
}

impl TryFrom<&ExecutionPayload> for HeadInfo {
    type Error = eyre::Report;

    fn try_from(payload: &ExecutionPayload) -> Result<Self> {
        let (epoch, seq_number) = payload
            .transactions
            .get(0)
            .ok_or(eyre::eyre!("no deposit transaction"))?
            .derive_unsafe_epoch()?;

        Ok(Self {
            head: BlockInfo {
                hash: payload.block_hash,
                number: payload.block_number.as_u64(),
                parent_hash: payload.parent_hash,
                timestamp: payload.timestamp.as_u64(),
            },
            epoch,
            seq_number,
        })
    }
}

impl TryFrom<Block<Transaction>> for HeadInfo {
    type Error = eyre::Report;

    fn try_from(block: Block<Transaction>) -> std::result::Result<Self, Self::Error> {
        let tx_calldata = block
            .transactions
            .get(0)
            .ok_or(eyre::eyre!(
                "Could not find the L1 attributes deposited transaction"
            ))?
            .input
            .clone();

        let call = AttributesDepositedCall::try_from(tx_calldata)?;

        Ok(Self {
            head: block.try_into()?,
            epoch: Epoch::from(&call),
            seq_number: call.sequence_number,
        })
    }
}

/// L1 epoch block
#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Epoch {
    pub number: u64,
    pub hash: H256,
    pub timestamp: u64,
}

impl From<Epoch> for Value {
    fn from(value: Epoch) -> Self {
        let mut dict: std::collections::BTreeMap<String, Value> = Dict::new();
        dict.insert("hash".to_string(), Value::from(value.hash.as_bytes()));
        dict.insert("number".to_string(), Value::from(value.number));
        dict.insert("timestamp".to_string(), Value::from(value.timestamp));
        Value::Dict(Tag::Default, dict)
    }
}

#[cfg(test)]
mod tests {
    use crate::types::common::HeadInfo;
    use std::str::FromStr;

    use ethers::{
        providers::{Middleware, Provider},
        types::{Block, Transaction, H256},
    };
    use eyre::Result;

    #[test]
    fn should_fail_conversion_from_a_block_to_head_info_if_missing_l1_deposited_tx() -> Result<()> {
        // Arrange
        let raw_block = r#"{
            "hash": "0x2e4f4aff36bb7951be9742ad349fb1db84643c6bbac5014f3d196fd88fe333eb",
            "parentHash": "0xeccf4c06ad0d27be1cadee5720a509d31a9de0462b52f2cf6045d9a73c9aa504",
            "sha3Uncles": "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
            "miner": "0x4200000000000000000000000000000000000011",
            "stateRoot": "0x5905b2423f299a29db41e377d7ceadf4baa49eed04e1b72957e8c0985e04e730",
            "transactionsRoot": "0x030e481411042a769edde83d790d583ed69f9d3098d4a78d00e008f749fcfd97",
            "receiptsRoot": "0x29079b696c12a19999f3bb303fddb6fc12fb701f427678cca24954b91080ada3",
            "number": "0x7fe52f",
            "gasUsed": "0xb711",
            "gasLimit": "0x17d7840",
            "extraData": "0x",
            "logsBloom": "0x00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            "timestamp": "0x644434c2",
            "difficulty": "0x0",
            "totalDifficulty": "0x0",
            "sealFields": [],
            "uncles": [],
            "transactions": [],
            "size": "0x365",
            "mixHash": "0x7aeec5550a9b0616701e49ab835af5f10eadba2a0582016f0e256c9cace0c046",
            "nonce": "0x0000000000000000",
            "baseFeePerGas": "0x32"
        }
        "#;

        let block: Block<Transaction> = serde_json::from_str(raw_block)?;

        // Act
        let head = HeadInfo::try_from(block);

        // Assert
        assert!(head.is_err());
        let err = head.unwrap_err();

        assert!(err
            .to_string()
            .contains("Could not find the L1 attributes deposited transaction"));

        Ok(())
    }

    #[test]
    fn should_convert_from_a_block_to_head_info() -> Result<()> {
        // Arrange
        let raw_block = r#"{
            "hash": "0x2e4f4aff36bb7951be9742ad349fb1db84643c6bbac5014f3d196fd88fe333eb",
            "parentHash": "0xeccf4c06ad0d27be1cadee5720a509d31a9de0462b52f2cf6045d9a73c9aa504",
            "sha3Uncles": "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
            "miner": "0x4200000000000000000000000000000000000011",
            "stateRoot": "0x5905b2423f299a29db41e377d7ceadf4baa49eed04e1b72957e8c0985e04e730",
            "transactionsRoot": "0x030e481411042a769edde83d790d583ed69f9d3098d4a78d00e008f749fcfd97",
            "receiptsRoot": "0x29079b696c12a19999f3bb303fddb6fc12fb701f427678cca24954b91080ada3",
            "number": "0x7fe52f",
            "gasUsed": "0xb711",
            "gasLimit": "0x17d7840",
            "extraData": "0x",
            "logsBloom": "0x00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            "timestamp": "0x644434c2",
            "difficulty": "0x0",
            "totalDifficulty": "0x0",
            "sealFields": [],
            "uncles": [],
            "transactions": [
            {
                "hash": "0x661df2908a63c9701ef4f9bc1d62432f08cbdc8c6fe6012af49405c00de5f69d",
                "nonce": "0x41ed06",
                "blockHash": "0x2e4f4aff36bb7951be9742ad349fb1db84643c6bbac5014f3d196fd88fe333eb",
                "blockNumber": "0x7fe52f",
                "transactionIndex": "0x0",
                "from": "0xdeaddeaddeaddeaddeaddeaddeaddeaddead0001",
                "to": "0x4200000000000000000000000000000000000015",
                "value": "0x0",
                "gasPrice": "0x0",
                "gas": "0xf4240",
                "input": "0x015d8eb900000000000000000000000000000000000000000000000000000000008768240000000000000000000000000000000000000000000000000000000064443450000000000000000000000000000000000000000000000000000000000000000e0444c991c5fe1d7291ff34b3f5c3b44ee861f021396d33ba3255b83df30e357d00000000000000000000000000000000000000000000000000000000000000050000000000000000000000007431310e026b69bfc676c0013e12a1a11411eec9000000000000000000000000000000000000000000000000000000000000083400000000000000000000000000000000000000000000000000000000000f4240",
                "v": "0x0",
                "r": "0x0",
                "s": "0x0",
                "type": "0x7e",
                "mint": "0x0",
                "sourceHash": "0x34ad504eea583add76d3b9d249965356ef6ca344d6766644c929357331bb0dc9"
            }
            ],
            "size": "0x365",
            "mixHash": "0x7aeec5550a9b0616701e49ab835af5f10eadba2a0582016f0e256c9cace0c046",
            "nonce": "0x0000000000000000",
            "baseFeePerGas": "0x32"
        }
        "#;

        let block: Block<Transaction> = serde_json::from_str(raw_block)?;

        let expected_l2_block_hash =
            H256::from_str("0x2e4f4aff36bb7951be9742ad349fb1db84643c6bbac5014f3d196fd88fe333eb")?;
        let expected_l2_block_number = 8381743;
        let expected_l2_block_timestamp = 1682191554;

        let expected_l1_epoch_hash =
            H256::from_str("0444c991c5fe1d7291ff34b3f5c3b44ee861f021396d33ba3255b83df30e357d")?;
        let expected_l1_epoch_block_number = 8874020;
        let expected_l1_epoch_timestamp = 1682191440;

        // Act
        let head = HeadInfo::try_from(block);

        // Assert
        assert!(head.is_ok());
        let HeadInfo {
            head,
            epoch,
            seq_number,
        } = head.unwrap();

        assert_eq!(head.hash, expected_l2_block_hash);
        assert_eq!(head.number, expected_l2_block_number);
        assert_eq!(head.timestamp, expected_l2_block_timestamp);

        assert_eq!(epoch.hash, expected_l1_epoch_hash);
        assert_eq!(epoch.number, expected_l1_epoch_block_number);
        assert_eq!(epoch.timestamp, expected_l1_epoch_timestamp);

        assert_eq!(seq_number, 5);

        Ok(())
    }

    #[tokio::test]
    async fn test_head_info_from_l2_block_hash() -> Result<()> {
        let l2_rpc = match std::env::var("L2_TEST_RPC_URL") {
            Ok(l2_rpc) => l2_rpc,
            l2_rpc_res => {
                eprintln!(
                    "Test ignored: `test_head_info_from_l2_block_hash`, l2_rpc: {l2_rpc_res:?}"
                );
                return Ok(());
            }
        };

        let l2_block_hash =
            H256::from_str("0x75d4a658d7b6430c874c5518752a8d90fb1503eccd6ae4cfc97fd4aedeebb939")?;

        let expected_l2_block_number = 8428108;
        let expected_l2_block_timestamp = 1682284284;

        let expected_l1_epoch_hash =
            H256::from_str("0x76ab90dc2afea158bbe14a99f22d5f867b51719378aa37d1a3aa3833ace67cad")?;
        let expected_l1_epoch_block_number = 8879997;
        let expected_l1_epoch_timestamp = 1682284164;

        let provider = Provider::try_from(l2_rpc)?;

        let l2_block = provider.get_block_with_txs(l2_block_hash).await?.unwrap();
        let head = HeadInfo::try_from(l2_block)?;

        let HeadInfo {
            head,
            epoch,
            seq_number,
        } = head;

        assert_eq!(head.number, expected_l2_block_number);
        assert_eq!(head.timestamp, expected_l2_block_timestamp);

        assert_eq!(epoch.hash, expected_l1_epoch_hash);
        assert_eq!(epoch.number, expected_l1_epoch_block_number);
        assert_eq!(epoch.timestamp, expected_l1_epoch_timestamp);

        assert_eq!(seq_number, 4);

        Ok(())
    }
}
