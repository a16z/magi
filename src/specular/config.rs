use ethers::types::{Address, H256, U256};
use eyre::Context;
use serde::{Deserialize, Serialize};

use crate::{
    common::{BlockInfo, Epoch},
    config::{ChainConfig, ProtocolMetaConfig, SystemConfig},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExternalChainConfig {
    genesis: ExternalGenesisInfo,
    block_time: u64,
    max_sequencer_drift: u64,
    seq_window_size: u64,
    l1_chain_id: u64,
    l2_chain_id: u64,
    batch_inbox_address: Address,
    l1_oracle_address: Address,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExternalGenesisInfo {
    l1: ChainGenesisInfo,
    l2: ChainGenesisInfo,
    l2_time: u64,
    system_config: SystemConfigInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SystemConfigInfo {
    #[serde(rename = "batcherAddr")]
    batcher_addr: Address,
    #[serde(rename = "gasLimit")]
    gas_limit: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChainGenesisInfo {
    hash: H256,
    number: u64,
}

impl ChainConfig {
    pub fn from_specular_json(path: &str) -> Self {
        let file = std::fs::File::open(path)
            .with_context(|| format!("Failed to read chain config from {}", path))
            .unwrap();
        let external: ExternalChainConfig = serde_json::from_reader(file).unwrap();
        external.into()
    }
}

impl From<ExternalChainConfig> for ChainConfig {
    fn from(external: ExternalChainConfig) -> Self {
        Self {
            network: "external".to_string(),
            l1_chain_id: external.l1_chain_id,
            l2_chain_id: external.l2_chain_id,
            l1_start_epoch: Epoch {
                hash: external.genesis.l1.hash,
                number: external.genesis.l1.number,
                timestamp: external.genesis.l2_time,
            },
            l2_genesis: BlockInfo {
                hash: external.genesis.l2.hash,
                number: external.genesis.l2.number,
                parent_hash: H256::zero(),
                timestamp: external.genesis.l2_time,
            },
            system_config: SystemConfig {
                batch_sender: external.genesis.system_config.batcher_addr,
                gas_limit: U256::from(external.genesis.system_config.gas_limit),
                l1_fee_overhead: U256::from(0),       // not used
                l1_fee_scalar: U256::from(0),         // not used
                unsafe_block_signer: Address::zero(), // not used?
            },
            batch_inbox: external.batch_inbox_address,
            deposit_contract: Address::zero(),         // not used
            system_config_contract: Address::zero(),   // not used
            max_channel_size: 0,                       // not used
            channel_timeout: 0,                        // not used
            seq_window_size: external.seq_window_size, // NOTE: not used in derivation, but used in `State`
            max_seq_drift: external.max_sequencer_drift,
            regolith_time: 0, // not used
            blocktime: external.block_time,
            l2_to_l1_message_passer: Address::zero(), // not used?
            meta: ProtocolMetaConfig::specular(external.l1_oracle_address),
        }
    }
}

impl ProtocolMetaConfig {
    pub fn specular(l1_oracle: Address) -> Self {
        Self {
            enable_config_updates: false,
            enable_deposited_txs: false,
            enable_full_derivation: false,
            l1_oracle,
        }
    }
}
