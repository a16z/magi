use ethers::types::{Block, Transaction};
use eyre::Result;
use serde::{Deserialize, Serialize};

use crate::common::{BlockInfo, Epoch};

/// Block info for the current head of the chain
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HeadInfo {
    /// L2 BlockInfo value
    pub l2_block_info: BlockInfo,
    /// L1 batch epoch of the head L2 block
    pub l1_epoch: Epoch,
}

impl TryFrom<Block<Transaction>> for HeadInfo {
    type Error = eyre::Report;

    fn try_from(value: Block<Transaction>) -> Result<Self> {
        let tx_calldata = value
            .transactions
            .get(0)
            .ok_or(eyre::eyre!(
                "Could not find the L1 attributes deposited transaction"
            ))?
            .input
            .clone();

        Ok(Self {
            l2_block_info: value.try_into()?,
            l1_epoch: tx_calldata.try_into()?,
        })
    }
}

#[cfg(test)]
mod tests {

    mod head_info {
        use std::str::FromStr;

        use ethers::types::{Block, Transaction, H256};

        use crate::driver::HeadInfo;

        #[test]
        fn should_fail_conversion_from_a_block_to_head_info_if_missing_l1_deposited_tx(
        ) -> eyre::Result<()> {
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
        fn should_convert_from_a_block_to_head_info() -> eyre::Result<()> {
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

            let expected_l2_block_hash = H256::from_str(
                "0x2e4f4aff36bb7951be9742ad349fb1db84643c6bbac5014f3d196fd88fe333eb",
            )?;
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
                l2_block_info,
                l1_epoch,
            } = head.unwrap();

            assert_eq!(l2_block_info.hash, expected_l2_block_hash);
            assert_eq!(l2_block_info.number, expected_l2_block_number);
            assert_eq!(l2_block_info.timestamp, expected_l2_block_timestamp);

            assert_eq!(l1_epoch.hash, expected_l1_epoch_hash);
            assert_eq!(l1_epoch.number, expected_l1_epoch_block_number);
            assert_eq!(l1_epoch.timestamp, expected_l1_epoch_timestamp);

            Ok(())
        }
    }
}
