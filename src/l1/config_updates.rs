use ethers::types::{Address, Log, U256};
use eyre::Result;

/// Represents a system config update event
#[derive(Debug)]
pub enum SystemConfigUpdate {
    /// The batch sender address has been updated
    BatchSender(Address),
    /// The fee overhead and scalar have been updated
    Fees(U256, U256),
    /// The gas has been updated
    Gas(U256),
    /// The unsafe block signer has been updated
    UnsafeBlockSigner(Address),
}

impl TryFrom<Log> for SystemConfigUpdate {
    type Error = eyre::Report;

    fn try_from(log: Log) -> Result<Self> {
        let version = log
            .topics
            .get(1)
            .ok_or(eyre::eyre!("invalid system config update"))?
            .to_low_u64_be();

        if version != 0 {
            return Err(eyre::eyre!("invalid system config update"));
        }

        let update_type = log
            .topics
            .get(2)
            .ok_or(eyre::eyre!("invalid system config update"))?
            .to_low_u64_be();

        match update_type {
            0 => {
                let addr_bytes = log
                    .data
                    .get(76..96)
                    .ok_or(eyre::eyre!("invalid system config update"))?;

                let addr = Address::from_slice(addr_bytes);
                Ok(Self::BatchSender(addr))
            }
            1 => {
                let fee_overhead = log
                    .data
                    .get(64..96)
                    .ok_or(eyre::eyre!("invalid system config update"))?;

                let fee_scalar = log
                    .data
                    .get(96..128)
                    .ok_or(eyre::eyre!("invalid system config update"))?;

                let fee_overhead = U256::from_big_endian(fee_overhead);
                let fee_scalar = U256::from_big_endian(fee_scalar);

                Ok(Self::Fees(fee_overhead, fee_scalar))
            }
            2 => {
                let gas_bytes = log
                    .data
                    .get(64..96)
                    .ok_or(eyre::eyre!("invalid system config update"))?;

                let gas = U256::from_big_endian(gas_bytes);
                Ok(Self::Gas(gas))
            }
            3 => {
                let addr_bytes = log
                    .data
                    .get(76..96)
                    .ok_or(eyre::eyre!("invalid system config update"))?;

                let addr = Address::from_slice(addr_bytes);
                Ok(Self::UnsafeBlockSigner(addr))
            }
            _ => Err(eyre::eyre!("invalid system config update")),
        }
    }
}
