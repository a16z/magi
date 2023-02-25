use std::str::FromStr;

use ethers_core::types::Address;

pub struct Config {
    pub l1_rpc: String,
    pub chain: ChainConfig,
}

pub struct ChainConfig {
    pub batch_sender: Address,
    pub batch_inbox: Address,
    pub deposit_contract: Address,
}

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
