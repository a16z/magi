use alloy_primitives::{Address, U256, U64};
use alloy_rpc_types::Log;
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
        let version = U64::from_be_bytes(
            **log
                .topics()
                .get(1)
                .ok_or(eyre::eyre!("invalid system config update"))?,
        );

        if !version.is_zero() {
            return Err(eyre::eyre!("invalid system config update"));
        }

        let update_type = U64::from_be_bytes(
            **log
                .topics()
                .get(2)
                .ok_or(eyre::eyre!("invalid system config update"))?,
        );

        let update_type: u64 = update_type.try_into()?;
        match update_type {
            0 => {
                let addr_bytes = log
                    .data()
                    .data
                    .get(76..96)
                    .ok_or(eyre::eyre!("invalid system config update"))?;

                let addr = Address::from_slice(addr_bytes);
                Ok(Self::BatchSender(addr))
            }
            1 => {
                let fee_overhead = log
                    .data()
                    .data
                    .get(64..96)
                    .ok_or(eyre::eyre!("invalid system config update"))?;

                let fee_scalar = log
                    .data()
                    .data
                    .get(96..128)
                    .ok_or(eyre::eyre!("invalid system config update"))?;

                let fee_overhead: [u8; 32] = fee_overhead.try_into()?;
                let fee_scalar: [u8; 32] = fee_scalar.try_into()?;
                let fee_overhead = U256::from_be_bytes(fee_overhead);
                let fee_scalar = U256::from_be_bytes(fee_scalar);

                Ok(Self::Fees(fee_overhead, fee_scalar))
            }
            2 => {
                let gas_bytes = log
                    .data()
                    .data
                    .get(64..96)
                    .ok_or(eyre::eyre!("invalid system config update"))?;

                let gas_bytes: [u8; 32] = gas_bytes.try_into()?;
                let gas = U256::from_be_bytes(gas_bytes);
                Ok(Self::Gas(gas))
            }
            3 => {
                let addr_bytes = log
                    .data()
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
