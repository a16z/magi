use ethers::types::H160;

use crate::config;

/// Sequencing policy configuration.
#[derive(Clone, Debug)]
pub struct Config {
    pub max_safe_lag: u64,
    pub max_seq_drift: u64,
    pub blocktime: u64,
    pub system_config: SystemConfig,
}

/// Subset of system configuration required by sequencing policy.
/// TODO: The system config may change over time; handle this.
#[derive(Clone, Debug)]
pub struct SystemConfig {
    pub batch_sender: H160,
    pub gas_limit: u64,
}

impl Config {
    pub fn new(config: &config::Config) -> Self {
        Self {
            max_safe_lag: config.local_sequencer.max_safe_lag,
            max_seq_drift: config.chain.max_seq_drift,
            blocktime: config.chain.blocktime,
            system_config: SystemConfig::new(&config.chain.system_config),
        }
    }
}

impl SystemConfig {
    pub fn new(config: &config::SystemConfig) -> Self {
        Self {
            batch_sender: config.batch_sender,
            gas_limit: config.gas_limit.as_u64(),
        }
    }
}
