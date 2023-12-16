use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use ethers::{
    middleware::SignerMiddleware,
    providers::Middleware,
    signers::{LocalWallet, Signer},
    types::{TransactionRequest, H256, U256, U64},
};
use eyre::Result;

use crate::{
    common::{BlockInfo, Epoch, RawTransaction},
    driver::sequencing::SequencingPolicy,
    engine::PayloadAttributes,
    l1::L1BlockInfo,
};

use crate::specular::{
    common::{SetL1OracleValuesInput, SET_L1_ORACLE_VALUES_ABI, SET_L1_ORACLE_VALUES_SELECTOR},
    config::SystemAccounts,
};

pub mod config;

pub struct AttributesBuilder<M> {
    config: config::Config,
    client: SignerMiddleware<M, LocalWallet>,
}

// TODO[zhe]: 'static works for Http provider and MockProvider, not sure if it works for all [Middleware]
// TODO[zhe]: has to be 'static because o/w [Middleware::fill_transaction] will complain about lifetime
impl<M: Middleware + 'static> AttributesBuilder<M> {
    pub fn new(config: config::Config, l2_provider: M) -> Self {
        let wallet = LocalWallet::try_from(config.sequencer_private_key.clone())
            .expect("invalid sequencer private key")
            .with_chain_id(config.l2_chain_id);
        let client = SignerMiddleware::new(l2_provider, wallet);
        Self { config, client }
    }

    /// Returns the next l2 block timestamp, given the `parent_block_timestamp`.
    fn next_timestamp(&self, parent_block_timestamp: u64) -> u64 {
        parent_block_timestamp + self.config.blocktime
    }

    /// Returns the drift bound on the next l2 block's timestamp.
    fn next_drift_bound(&self, curr_origin: &L1BlockInfo) -> u64 {
        curr_origin.timestamp + self.config.max_seq_drift
    }

    /// Finds the origin of the next L2 block: either the current origin or the next, if sufficient time has passed.
    async fn find_next_origin(
        &self,
        curr_l2_block: &BlockInfo,
        curr_l1_epoch: &L1BlockInfo,
        next_l1_epoch: Option<&L1BlockInfo>,
    ) -> Result<L1BlockInfo> {
        let next_l2_ts = self.next_timestamp(curr_l2_block.timestamp);
        let next_drift_bound = self.next_drift_bound(curr_l1_epoch);
        let is_drift_bound_exceeded = next_l2_ts > next_drift_bound;
        if is_drift_bound_exceeded {
            tracing::info!("next l2 ts exceeds the drift bound {}", next_drift_bound);
        }
        match (next_l1_epoch, is_drift_bound_exceeded) {
            // We found the next l1 block.
            (Some(next_l1_epoch), _) => {
                if next_l2_ts >= next_l1_epoch.timestamp {
                    Ok(next_l1_epoch.clone())
                } else {
                    Ok(curr_l1_epoch.clone())
                }
            }
            // We exceeded the drift bound, so we can't use the current origin.
            // But we also can't use the next l1 block since we don't have it.
            (_, true) => Err(eyre::eyre!("current origin drift bound exceeded.")),
            // We're not exceeding the drift bound, so we can just use the current origin.
            (_, false) => {
                tracing::info!("Falling back to current origin (next is unknown).");
                Ok(curr_l1_epoch.clone())
            }
        }
    }

    // Creates the `L1Oracle::setL1OracleValues` transaction to include at the top of
    // the next l2 block, which marks the start of an epoch.
    async fn create_l1_oracle_update_transaction(
        &self,
        parent_l1_epoch: &L1BlockInfo,
        origin: &L1BlockInfo,
    ) -> Result<Vec<RawTransaction>> {
        if parent_l1_epoch.number == origin.number {
            // Do not include the L1 oracle update tx if we are still in the same L1 epoch.
            return Ok(vec![]);
        }
        // Construct L1 oracle update transaction data
        let set_l1_oracle_values_input: SetL1OracleValuesInput = (
            U256::from(origin.number),
            U256::from(origin.timestamp),
            origin.base_fee,
            origin.hash,
            origin.state_root,
            self.config.system_config.l1_fee_overhead,
            self.config.system_config.l1_fee_scalar,
        );
        let input = SET_L1_ORACLE_VALUES_ABI
            .encode_with_selector(*SET_L1_ORACLE_VALUES_SELECTOR, set_l1_oracle_values_input)
            .expect("failed to encode setL1OracleValues input");
        // Construct L1 oracle update transaction
        let mut tx = TransactionRequest::new()
            .to(SystemAccounts::default().l1_oracle)
            .gas(15_000_000) // TODO[zhe]: consider to lower this number or make it configurable
            .value(0)
            .data(input)
            .into();
        // TODO[zhe]: here we let the provider to fill in the gas price, consider to make it constant?
        // Currently `get_attributes` is always called with `parent_l2_block` being the latest block, see src/driver/sequencing/mod.rs:51.
        // Therefore, we can assume we're at the latest block and can fill on `Pending` block
        self.client.fill_transaction(&mut tx, None).await?;
        let signature = Signer::sign_transaction(self.client.signer(), &tx).await?;
        let raw_tx = tx.rlp_signed(&signature);
        Ok(vec![RawTransaction(raw_tx.0.into())])
    }
}

#[async_trait]
impl<M: Middleware + 'static> SequencingPolicy for AttributesBuilder<M> {
    /// Returns true iff (1a or 1b) AND (2):
    /// (1a) `parent_l2_block` is within the max safe lag (i.e. `parent_l2_block` isn't too far ahead of `safe_l2_head`).
    /// (1b) `max_safe_lag` is not configured (i.e. is 0).
    /// (2) The next timestamp isn't in the future.
    fn is_ready(&self, parent_l2_block: &BlockInfo, safe_l2_head: &BlockInfo) -> bool {
        let within_lag = self.config.max_safe_lag == 0
            || safe_l2_head.number + self.config.max_safe_lag > parent_l2_block.number;
        let precedes_future = self.next_timestamp(parent_l2_block.timestamp) <= unix_now();
        within_lag && precedes_future
    }

    async fn get_attributes(
        &self,
        parent_l2_block: &BlockInfo,
        parent_l1_epoch: &L1BlockInfo,
        next_l1_epoch: Option<&L1BlockInfo>,
    ) -> Result<PayloadAttributes> {
        let next_origin = self
            .find_next_origin(parent_l2_block, parent_l1_epoch, next_l1_epoch)
            .await?;
        let timestamp = self.next_timestamp(parent_l2_block.timestamp);
        let prev_randao = next_randao(&next_origin);
        let suggested_fee_recipient = self.config.system_config.batch_sender;
        let txs = self
            .create_l1_oracle_update_transaction(parent_l1_epoch, &next_origin)
            .await?;
        let no_tx_pool = timestamp > next_origin.timestamp + self.config.max_seq_drift;
        let gas_limit = self.config.system_config.gas_limit;
        Ok(PayloadAttributes {
            timestamp: U64::from(timestamp),
            prev_randao,
            suggested_fee_recipient,
            transactions: Some(txs),
            no_tx_pool,
            gas_limit: U64::from(gas_limit),
            epoch: Some(create_epoch(next_origin)),
            l1_inclusion_block: None,
            seq_number: None,
        })
    }
}

/// Returns the next l2 block randao, reusing that of the `next_origin`.
fn next_randao(next_origin: &L1BlockInfo) -> H256 {
    next_origin.mix_hash
}

/// Extracts the epoch information from `info`.
fn create_epoch(info: L1BlockInfo) -> Epoch {
    Epoch {
        number: info.number,
        hash: info.hash,
        timestamp: info.timestamp,
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use crate::{common::BlockInfo, driver::sequencing::SequencingPolicy};

    use super::{config, unix_now, AttributesBuilder};
    use ethers::{
        abi::Address,
        providers::{MockProvider, Provider},
        types::U256,
    };
    use eyre::Result;
    #[test]
    fn test_is_ready() -> Result<()> {
        // Setup.
        let config = config::Config {
            blocktime: 2,
            max_seq_drift: 0, // anything
            max_safe_lag: 10,
            l2_chain_id: 0, // anything
            system_config: config::SystemConfig {
                batch_sender: Address::zero(),
                gas_limit: 1,
                l1_fee_overhead: U256::from(4242),
                l1_fee_scalar: U256::from(1_000_000),
            }, // anything
            // random publicly known private key
            sequencer_private_key:
                "4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318".to_string(),
        };
        let mock_client = MockProvider::default();
        let provider: Provider<MockProvider> = Provider::new(mock_client);
        let attrs_builder = AttributesBuilder::new(config.clone(), provider);
        // Run test cases.
        let cases = vec![(true, true), (true, false), (false, true), (false, false)];
        for case in cases.iter() {
            let (input, expected) = generate_is_ready_case(case.0, case.1, config.clone());
            assert_eq!(
                attrs_builder.is_ready(&input.0, &input.1),
                expected,
                "case: {:?}",
                case
            );
        }
        Ok(())
    }

    /// Generates an (input, expected-output) test-case pair for `is_ready`.
    fn generate_is_ready_case(
        exceeds_lag: bool,
        exceeds_present: bool,
        config: config::Config,
    ) -> ((BlockInfo, BlockInfo), bool) {
        let now = unix_now();
        let parent_info = BlockInfo {
            number: if exceeds_lag {
                config.max_safe_lag
            } else {
                config.max_safe_lag - 1
            },
            hash: Default::default(),
            parent_hash: Default::default(),
            timestamp: if exceeds_present {
                now
            } else {
                now - config.blocktime
            },
        };
        let safe_head = BlockInfo {
            number: 0,
            hash: Default::default(),
            parent_hash: Default::default(),
            timestamp: 0,
        };
        ((parent_info, safe_head), !exceeds_lag && !exceeds_present)
    }
}
