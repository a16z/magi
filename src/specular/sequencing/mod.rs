use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use ethers::types::{H256, U64};
use eyre::Result;

use crate::{
    common::{BlockInfo, Epoch, RawTransaction},
    driver::sequencing::SequencingPolicy,
    engine::PayloadAttributes,
    l1::L1BlockInfo,
};

pub mod config;

pub struct AttributesBuilder {
    config: config::Config,
}

impl AttributesBuilder {
    pub fn new(config: config::Config) -> Self {
        Self { config }
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
            tracing::info!("Next l2 ts exceeds the drift bound {}", next_drift_bound);
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
}

#[async_trait]
impl SequencingPolicy for AttributesBuilder {
    /// Returns true iff:
    /// 1. `parent_l2_block` is within the max safe lag (i.e. the unsafe head isn't too far ahead of the safe head).
    /// 2. The next timestamp isn't in the future.
    fn is_ready(&self, parent_l2_block: &BlockInfo, safe_l2_head: &BlockInfo) -> bool {
        safe_l2_head.number + self.config.max_safe_lag > parent_l2_block.number
            && self.next_timestamp(parent_l2_block.timestamp) <= unix_now()
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
        let suggested_fee_recipient = self.config.suggested_fee_recipient;
        let txs = create_top_of_block_transactions(&next_origin);
        let no_tx_pool = timestamp > self.config.max_seq_drift;
        let gas_limit = self.config.system_config.gas_limit;
        Ok(PayloadAttributes {
            timestamp: U64([timestamp]),
            prev_randao,
            suggested_fee_recipient,
            transactions: Some(txs),
            no_tx_pool,
            gas_limit: U64([gas_limit]),
            epoch: Some(create_epoch(next_origin)),
            l1_inclusion_block: None,
            seq_number: None,
        })
    }
}

// TODO: implement. requires l1 info tx. requires signer...
// Creates the transaction(s) to include at the top of the next l2 block.
fn create_top_of_block_transactions(_origin: &L1BlockInfo) -> Vec<RawTransaction> {
    vec![]
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
    use eyre::Result;

    #[test]
    fn test_is_ready() -> Result<()> {
        // Setup.
        let config = config::Config {
            blocktime: 2,
            max_seq_drift: 0, // anything
            max_safe_lag: 10,
            suggested_fee_recipient: Default::default(), // anything
            system_config: config::SystemConfig { gas_limit: 1 }, // anything
        };
        let attrs_builder = AttributesBuilder::new(config.clone());
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
