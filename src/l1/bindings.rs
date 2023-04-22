use ethers::{
    prelude::abigen,
    providers::{Http, Provider, RetryClient},
    types::{Address, U256},
};
use eyre::Result;

use crate::config::Config;

use super::generate_http_provider;

abigen! {
    OptimismPortal,
    r#"[
        function GUARDIAN() external view returns (address)
        function L2_ORACLE() external view returns (address)
        function SYSTEM_CONFIG() external view returns (address)
    ]"#,
}

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

pub struct L1Bindings<T> {
    optimism_portal: OptimismPortal<T>,
    l2_output_oracle: L2OutputOracle<T>,
}

impl L1Bindings<Provider<RetryClient<Http>>> {
    pub async fn from_config(config: &Config) -> Result<Self> {
        let provider = generate_http_provider(&config.l1_rpc_url);

        let op_portal_address = config.chain.portal;
        let optimism_portal = OptimismPortal::new(op_portal_address, provider.clone());

        let l2_oracle_address = optimism_portal.l2_oracle().call().await?;
        let l2_output_oracle = L2OutputOracle::new(l2_oracle_address, provider);

        Ok(Self {
            optimism_portal,
            l2_output_oracle,
        })
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
    use std::{path::PathBuf, sync::Arc};

    use super::*;
    use crate::config::{ChainConfig, Config};

    #[tokio::test]
    async fn test_get_latest_l2_output() {
        let rpc = "https://eth-goerli.g.alchemy.com/v2/a--NIcyeycPntQX42kunxUIVkg6_ekYc";
        let config = Arc::new(Config {
            l1_rpc_url: rpc.to_string(),
            l2_rpc_url: "mocked".to_string(),
            chain: ChainConfig::optimism_goerli(),
            data_dir: PathBuf::default(),
            l2_engine_url: String::new(),
            jwt_secret: String::new(),
        });

        let l1_bindings = L1Bindings::from_config(&config).await.unwrap();

        let (latest_index, latest_output) = l1_bindings.get_latest_l2_output().await.unwrap();

        println!("latest_index: {}", latest_index);
        println!("latest_output: {:?}", latest_output);
    }

    #[tokio::test]
    async fn test_get_l2_output() {
        let rpc = "https://eth-goerli.g.alchemy.com/v2/a--NIcyeycPntQX42kunxUIVkg6_ekYc";
        let config = Arc::new(Config {
            l1_rpc_url: rpc.to_string(),
            l2_rpc_url: "mocked".to_string(),
            chain: ChainConfig::optimism_goerli(),
            data_dir: PathBuf::default(),
            l2_engine_url: String::new(),
            jwt_secret: String::new(),
        });

        let l1_bindings = L1Bindings::from_config(&config).await.unwrap();

        let l2_output_index = U256::from(0);
        let output = l1_bindings.get_l2_output(l2_output_index).await.unwrap();

        println!("output: {:?}", output);
    }
}
