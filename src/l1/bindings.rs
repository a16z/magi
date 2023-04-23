use ethers::{
    prelude::abigen,
    providers::{Http, Provider, RetryClient},
    types::{H256, U256},
    utils::keccak256,
};
use eyre::Result;
use once_cell::sync::Lazy;

use crate::config::Config;

use super::generate_http_provider;

abigen! {
    L2OutputOracle,
    r#"[
        function SUBMISSION_INTERVAL() external view returns (uint256)
        function L2_BLOCK_TIME() external view returns (uint256)
        function CHALLENGER() external view returns (address)
        function PROPOSER() external view returns (address)
        function FINALIZATION_PERIOD_SECONDS() external view returns (uint256)
        function startingBlockNumber() external view returns (uint256)
        function startingTimestamp() external view returns (uint256)
        function latestBlockNumber() public view returns (uint256)
        function nextBlockNumber() public view returns (uint256)
        function latestOutputIndex() external view returns (uint256)
        function getL2Output(uint256 _l2OutputIndex) external view returns (OutputProposal)
        function getL2OutputAfter(uint256 _l2BlockNumber) external view returns (OutputProposal)
        struct OutputProposal { bytes32 outputRoot; uint128 timestamp; uint128 l2BlockNumber; }
        event OutputProposed(bytes32 indexed outputRoot, uint256 indexed l2OutputIndex, uint256 indexed l2BlockNumber,uint256 l1Timestamp)
    ]"#,
}

pub static OUTPUT_PROPOSED_TOPIC: Lazy<H256> = Lazy::new(|| {
    H256::from_slice(&keccak256(
        "OutputProposed(bytes32,uint256,uint256,uint256)",
    ))
});

pub struct L1Bindings {
    l2_output_oracle: L2OutputOracle<Provider<RetryClient<Http>>>,
}

impl L1Bindings {
    pub fn from_config(config: &Config) -> Self {
        let provider = generate_http_provider(&config.l1_rpc_url);
        let l2_output_oracle = L2OutputOracle::new(config.chain.l2_output_oracle, provider);

        Self { l2_output_oracle }
    }

    pub async fn get_l2_output(&self, l2_output_index: U256) -> Result<OutputProposal> {
        let (output_root, timestamp, l_2_block_number) = self
            .l2_output_oracle
            .get_l2_output(l2_output_index)
            .call()
            .await?;

        Ok(OutputProposal {
            output_root,
            timestamp,
            l_2_block_number,
        })
    }

    /// Returns a tuple with the latest output index and its corresponding output proposal.
    pub async fn get_latest_l2_output(&self) -> Result<(U256, OutputProposal)> {
        let latest_index = self.l2_output_oracle.latest_output_index().call().await?;
        Ok((latest_index, self.get_l2_output(latest_index).await?))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::config::{ChainConfig, Config};

    #[tokio::test]
    async fn test_get_latest_l2_output() {
        let rpc = "https://eth-goerli.g.alchemy.com/v2/ptMIwA5DSr2c0Pc-EI6-9AGnILcb0tts";
        let config = Arc::new(Config {
            l1_rpc_url: rpc.to_string(),
            l2_rpc_url: "mocked".to_string(),
            chain: ChainConfig::optimism_goerli(),
            l2_engine_url: String::new(),
            jwt_secret: String::new(),
        });

        let l1_bindings = L1Bindings::from_config(&config);

        let (_, latest_output) = l1_bindings.get_latest_l2_output().await.unwrap();

        assert_eq!(latest_output.output_root.len(), 32);
    }

    #[tokio::test]
    async fn test_get_l2_output() {
        let rpc = "https://eth-goerli.g.alchemy.com/v2/ptMIwA5DSr2c0Pc-EI6-9AGnILcb0tts";
        let config = Arc::new(Config {
            l1_rpc_url: rpc.to_string(),
            l2_rpc_url: "mocked".to_string(),
            chain: ChainConfig::optimism_goerli(),
            l2_engine_url: String::new(),
            jwt_secret: String::new(),
        });

        let l1_bindings = L1Bindings::from_config(&config);

        let l2_output_index = U256::from(0);
        let output = l1_bindings.get_l2_output(l2_output_index).await.unwrap();

        assert_eq!(output.output_root.len(), 32);
        assert_eq!(
            output.output_root,
            [
                41, 43, 160, 214, 97, 209, 16, 252, 28, 153, 239, 186, 51, 22, 169, 224, 124, 34,
                225, 145, 227, 216, 168, 200, 168, 98, 240, 160, 78, 134, 18, 190
            ]
        );
        assert_eq!(output.timestamp, 1673556960);
        assert_eq!(output.l_2_block_number, 4061236);
    }
}
