use std::str::FromStr;

use ethers_core::types::Address;

/// A system configuration
#[derive(Debug, Clone)]
pub struct Config {
    /// The base chain RPC URL
    pub base_chain_rpc: String,
    /// The base chain config
    pub chain: ChainConfig,
    /// The maximum number of intermediate pending channels
    pub max_channels: usize,
    /// The max timeout for a channel (as measured by the frame L1 block number)
    pub max_timeout: u64,
}

/// A Chain Configuration
#[derive(Debug, Clone)]
pub struct ChainConfig {
    /// The batch sender address
    pub batch_sender: Address,
    /// The batch inbox address
    pub batch_inbox: Address,
    /// The deposit contract address
    pub deposit_contract: Address,
}

/// System accounts
#[derive(Debug, Clone)]
pub struct SystemAccounts {
    pub attributes_depositor: Address,
    pub attributes_predeploy: Address,
    pub fee_vault: Address,
}

impl ChainConfig {
    pub fn goerli() -> Self {
        Self {
            batch_sender: addr("0x7431310e026b69bfc676c0013e12a1a11411eec9"),
            batch_inbox: addr("0xff00000000000000000000000000000000000420"),
            deposit_contract: addr("0x5b47E1A08Ea6d985D6649300584e6722Ec4B1383"),
        }
    }
}

impl Default for SystemAccounts {
    fn default() -> Self {
        Self {
            attributes_depositor: addr("0xdeaddeaddeaddeaddeaddeaddeaddeaddead0001"),
            attributes_predeploy: addr("0x4200000000000000000000000000000000000015"),
            fee_vault: addr("0x4200000000000000000000000000000000000011"),
        }
    }
}

fn addr(s: &str) -> Address {
    Address::from_str(s).unwrap()
}
