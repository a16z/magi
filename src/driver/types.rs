//! A module to handle conversions to a [HeadInfo] struct.

use alloy_rpc_types::{Block, BlockTransactions};
use eyre::Result;
use serde::{Deserialize, Serialize};

use crate::{
    common::{AttributesDepositedCall, BlockInfo, Epoch},
    config::Config,
};

/// Block info for the current head of the chain
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HeadInfo {
    /// L2 BlockInfo value
    pub l2_block_info: BlockInfo,
    /// L1 batch epoch of the head L2 block
    pub l1_epoch: Epoch,
    /// Sequencer number of head block
    pub sequence_number: u64,
}

impl HeadInfo {
    /// Returns the head info from the given L2 block and the system config.
    /// The config is used to check whether the block is subject to the Ecotone hardfork
    /// (which changes the way the head info is constructed from the block).
    pub fn try_from_l2_block(config: &Config, l2_block: Block) -> Result<Self> {
        if config
            .chain
            .is_ecotone_but_not_first_block(l2_block.header.timestamp)
        {
            HeadInfo::try_from_ecotone_block(l2_block)
        } else {
            HeadInfo::try_from_bedrock_block(l2_block)
        }
    }

    /// Returns `HeadInfo` consisting of the L2 block, the L1 epoch block it belongs to, and the L2 block's position in the epoch.
    /// This function is used when the L2 block is from the Bedrock hardfork or earlier.
    pub fn try_from_bedrock_block(block: Block) -> Result<Self> {
        let BlockTransactions::Full(txs) = block.transactions else {
            return Err(eyre::eyre!(
                "Could not find the L1 attributes deposited transaction"
            ));
        };
        let Some(first_tx) = txs.first() else {
            return Err(eyre::eyre!(
                "Could not find the L1 attributes deposited transaction"
            ));
        };

        let tx_calldata = first_tx.input.to_vec();
        let call = AttributesDepositedCall::try_from_bedrock(tx_calldata.into())?;

        Ok(Self {
            l2_block_info: BlockInfo::try_from(block)?,
            l1_epoch: Epoch::from(&call),
            sequence_number: call.sequence_number,
        })
    }

    /// Returns `HeadInfo` consisting of the L2 block, the L1 epoch block it belongs to, and the L2 block's position in the epoch.
    /// This function is used when the L2 block is from the Ecotone hardfork or later.
    pub fn try_from_ecotone_block(block: Block) -> Result<Self> {
        let BlockTransactions::Full(txs) = block.transactions else {
            return Err(eyre::eyre!(
                "Could not find the L1 attributes deposited transaction"
            ));
        };
        let Some(first_tx) = txs.first() else {
            return Err(eyre::eyre!(
                "Could not find the L1 attributes deposited transaction"
            ));
        };

        let tx_calldata = first_tx.input.to_vec();
        let call = AttributesDepositedCall::try_from_ecotone(tx_calldata.into())?;

        Ok(Self {
            l2_block_info: BlockInfo::try_from(block)?,
            l1_epoch: Epoch::from(&call),
            sequence_number: call.sequence_number,
        })
    }
}

#[cfg(test)]
mod tests {
    mod head_info_bedrock {
        use crate::driver::HeadInfo;
        use alloy_primitives::b256;
        use alloy_provider::{Provider, ProviderBuilder};
        use alloy_rpc_types::Block;
        use eyre::Result;

        #[test]
        fn should_fail_conversion_from_a_block_to_head_info_if_missing_l1_deposited_tx(
        ) -> Result<()> {
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

            let block: Block = serde_json::from_str(raw_block)?;

            let head = HeadInfo::try_from_bedrock_block(block);
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

            let block: Block = serde_json::from_str(raw_block)?;

            let expected_l2_block_hash =
                b256!("2e4f4aff36bb7951be9742ad349fb1db84643c6bbac5014f3d196fd88fe333eb");
            let expected_l2_block_number = 8381743;
            let expected_l2_block_timestamp = 1682191554;

            let expected_l1_epoch_hash =
                b256!("0444c991c5fe1d7291ff34b3f5c3b44ee861f021396d33ba3255b83df30e357d");
            let expected_l1_epoch_block_number = 8874020;
            let expected_l1_epoch_timestamp = 1682191440;

            let head = HeadInfo::try_from_bedrock_block(block);

            assert!(head.is_ok());
            let HeadInfo {
                l2_block_info,
                l1_epoch,
                sequence_number,
            } = head.unwrap();

            assert_eq!(l2_block_info.hash, expected_l2_block_hash);
            assert_eq!(l2_block_info.number, expected_l2_block_number);
            assert_eq!(l2_block_info.timestamp, expected_l2_block_timestamp);

            assert_eq!(l1_epoch.hash, expected_l1_epoch_hash);
            assert_eq!(l1_epoch.number, expected_l1_epoch_block_number);
            assert_eq!(l1_epoch.timestamp, expected_l1_epoch_timestamp);

            assert_eq!(sequence_number, 5);

            Ok(())
        }

        #[tokio::test]
        async fn test_head_info_from_l2_block_hash() -> Result<()> {
            let Ok(l1_rpc_url) = std::env::var("L1_TEST_RPC_URL") else {
                eyre::bail!("L1_TEST_RPC_URL is not set");
            };
            let Ok(l2_rpc_url) = std::env::var("L2_TEST_RPC_URL") else {
                eyre::bail!("L2_TEST_RPC_URL is not set");
            };

            let l2_block_hash =
                b256!("75d4a658d7b6430c874c5518752a8d90fb1503eccd6ae4cfc97fd4aedeebb939");

            let expected_l2_block_number = 8428108;
            let expected_l2_block_timestamp = 1682284284;

            let expected_l1_epoch_hash =
                b256!("76ab90dc2afea158bbe14a99f22d5f867b51719378aa37d1a3aa3833ace67cad");
            let expected_l1_epoch_block_number = 8879997;
            let expected_l1_epoch_timestamp = 1682284164;

            let l2_rpc_url = reqwest::Url::parse(&l2_rpc_url)?;
            let l2_provider = ProviderBuilder::new().on_http(l2_rpc_url);

            let l2_block = l2_provider
                .get_block(l2_block_hash.into(), true)
                .await?
                .unwrap();
            let head = HeadInfo::try_from_bedrock_block(l2_block)?;

            let HeadInfo {
                l2_block_info,
                l1_epoch,
                sequence_number,
            } = head;

            assert_eq!(l2_block_info.number, expected_l2_block_number);
            assert_eq!(l2_block_info.timestamp, expected_l2_block_timestamp);

            assert_eq!(l1_epoch.hash, expected_l1_epoch_hash);
            assert_eq!(l1_epoch.number, expected_l1_epoch_block_number);
            assert_eq!(l1_epoch.timestamp, expected_l1_epoch_timestamp);

            assert_eq!(sequence_number, 4);

            Ok(())
        }
    }

    mod head_info_ecotone {
        use crate::driver::HeadInfo;
        use alloy_primitives::b256;
        use alloy_provider::{Provider, ProviderBuilder};
        use eyre::Result;

        #[tokio::test]
        async fn test_head_info_from_l2_block_hash() -> Result<()> {
            let Ok(l1_rpc_url) = std::env::var("L1_TEST_RPC_URL") else {
                eyre::bail!("L1_TEST_RPC_URL is not set");
            };
            let Ok(l2_rpc_url) = std::env::var("L2_TEST_RPC_URL") else {
                eyre::bail!("L2_TEST_RPC_URL is not set");
            };

            let l2_block_hash =
                b256!("bdd36f0c7ec8e17647dac2780130a842c47dba0025387e80408c161fdb115412");

            let expected_l2_block_number = 21564471;
            let expected_l2_block_timestamp = 1708557010;

            let expected_l1_epoch_hash =
                b256!("231ac984a3fc58757efe373b0b5bff0589c0e67a969a6cbc56ec959739525b31");
            let expected_l1_epoch_block_number = 10575507;
            let expected_l1_epoch_timestamp = 1708556928;

            let l2_rpc_url = reqwest::Url::parse(&l2_rpc_url)?;
            let l2_provider = ProviderBuilder::new().on_http(l2_rpc_url);

            let l2_block = l2_provider
                .get_block(l2_block_hash.into(), true)
                .await?
                .unwrap();
            let head = HeadInfo::try_from_ecotone_block(l2_block)?;

            let HeadInfo {
                l2_block_info,
                l1_epoch,
                sequence_number,
            } = head;

            assert_eq!(l2_block_info.number, expected_l2_block_number);
            assert_eq!(l2_block_info.timestamp, expected_l2_block_timestamp);

            assert_eq!(l1_epoch.hash, expected_l1_epoch_hash);
            assert_eq!(l1_epoch.number, expected_l1_epoch_block_number);
            assert_eq!(l1_epoch.timestamp, expected_l1_epoch_timestamp);

            assert_eq!(sequence_number, 4);

            Ok(())
        }
    }
}
