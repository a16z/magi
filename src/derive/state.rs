//! A module that keeps track of the current derivation state.
//! The [State] caches previous L1 and L2 blocks.

use std::{collections::BTreeMap, sync::Arc};

use alloy_primitives::B256;
use alloy_provider::Provider;

use crate::{
    common::{BlockInfo, Epoch},
    config::Config,
    driver::HeadInfo,
    l1::L1Info,
};

/// Represents the current derivation state.
/// Consists of cached L1 & L2 blocks, and details of the current safe head & safe epoch.
pub struct State {
    /// Map of L1 blocks from the current L1 safe epoch - ``seq_window_size``
    l1_info: BTreeMap<B256, L1Info>,
    /// Map of L1 block hashes from the current L1 safe epoch - ``seq_window_size``
    l1_hashes: BTreeMap<u64, B256>,
    /// Map of L2 blocks from the current L2 safe head - (``max_seq_drift`` / ``blocktime``)
    l2_refs: BTreeMap<u64, (BlockInfo, Epoch)>,
    /// The current safe head
    pub safe_head: BlockInfo,
    /// The current safe epoch
    pub safe_epoch: Epoch,
    /// The current epoch number. Same as the first L1 block number in this sequencing window.
    pub current_epoch_num: u64,
    /// Global config
    config: Arc<Config>,
}

impl State {
    /// Creates a new [State] and fetches and caches a range of L2 blocks.
    pub async fn new(
        finalized_head: BlockInfo,
        finalized_epoch: Epoch,
        provider: &dyn Provider,
        config: Arc<Config>,
    ) -> Self {
        let l2_refs = l2_refs(finalized_head.number, provider, &config).await;

        Self {
            l1_info: BTreeMap::new(),
            l1_hashes: BTreeMap::new(),
            l2_refs,
            safe_head: finalized_head,
            safe_epoch: finalized_epoch,
            current_epoch_num: 0,
            config,
        }
    }

    /// Returns a cached L1 block by block hash
    pub fn l1_info_by_hash(&self, hash: B256) -> Option<&L1Info> {
        self.l1_info.get(&hash)
    }

    /// Returns a cached L1 block by block number
    pub fn l1_info_by_number(&self, num: u64) -> Option<&L1Info> {
        self.l1_hashes
            .get(&num)
            .and_then(|hash| self.l1_info.get(hash))
    }

    /// Returns a cached L2 block by block timestamp
    pub fn l2_info_by_timestamp(&self, timestamp: u64) -> Option<&(BlockInfo, Epoch)> {
        let block_num = (timestamp - self.config.chain.l2_genesis.timestamp)
            / self.config.chain.blocktime
            + self.config.chain.l2_genesis.number;

        self.l2_refs.get(&block_num)
    }

    /// Returns an epoch from an L1 block hash
    pub fn epoch_by_hash(&self, hash: B256) -> Option<Epoch> {
        self.l1_info_by_hash(hash).map(|info| Epoch {
            number: info.block_info.number,
            hash: info.block_info.hash,
            timestamp: info.block_info.timestamp,
        })
    }

    /// Returns an epoch by number. Same as the first L1 block number in the epoch's sequencing window.
    pub fn epoch_by_number(&self, num: u64) -> Option<Epoch> {
        self.l1_info_by_number(num).map(|info| Epoch {
            number: info.block_info.number,
            hash: info.block_info.hash,
            timestamp: info.block_info.timestamp,
        })
    }

    /// Inserts data from the ``l1_info`` parameter into ``l1_hashes`` & ``l1_info`` maps.
    ///
    /// This also updates ``current_epoch_num`` to the block number of the given ``l1_info``.
    pub fn update_l1_info(&mut self, l1_info: L1Info) {
        self.current_epoch_num = l1_info.block_info.number;
        self.l1_hashes.insert(
            l1_info.block_info.number,
            l1_info.block_info.hash,
        );
        self.l1_info.insert(l1_info.block_info.hash, l1_info);
        self.prune();
    }

    /// Resets the state and updates the safe head with the given parameters.
    ///
    /// ``current_epoch_num`` is set to 0.
    ///
    /// ``l1_info`` & ``l1_hashes`` mappings are cleared.
    pub fn purge(&mut self, safe_head: BlockInfo, safe_epoch: Epoch) {
        self.current_epoch_num = 0;
        self.l1_info.clear();
        self.l1_hashes.clear();

        self.update_safe_head(safe_head, safe_epoch);
    }

    /// Sets ``safe_head`` & ``safe_epoch`` to the given parameters.
    ///
    /// Also inserts these details into ``l2_refs``.
    pub fn update_safe_head(&mut self, safe_head: BlockInfo, safe_epoch: Epoch) {
        self.safe_head = safe_head;
        self.safe_epoch = safe_epoch;

        self.l2_refs
            .insert(self.safe_head.number, (self.safe_head, self.safe_epoch));
    }

    /// Removes keys from ``l1_info`` & ``l1_hashes`` mappings if older than ``self.safe_epoch.number`` - ``seq_window_size``.
    ///
    /// Removes keys from the ``l2_refs`` mapping if older than ``self.safe_head.number`` - (``max_seq_drift`` / ``blocktime``)
    fn prune(&mut self) {
        let prune_until = self
            .safe_epoch
            .number
            .saturating_sub(self.config.chain.seq_window_size);

        while let Some((block_num, block_hash)) = self.l1_hashes.first_key_value() {
            if *block_num >= prune_until {
                break;
            }

            self.l1_info.remove(block_hash);
            self.l1_hashes.pop_first();
        }

        let prune_until =
            self.safe_head.number - self.config.chain.max_seq_drift / self.config.chain.blocktime;

        while let Some((num, _)) = self.l2_refs.first_key_value() {
            if *num >= prune_until {
                break;
            }

            self.l2_refs.pop_first();
        }
    }
}

/// Returns the L2 blocks from the given ``head_num`` - (``max_seq_drift`` / ``blocktime``) to ``head_num``.
///
/// If the lookback period is before the genesis block, it will return L2 blocks starting from genesis.
async fn l2_refs(
    head_num: u64,
    provider: &dyn Provider,
    config: &Config,
) -> BTreeMap<u64, (BlockInfo, Epoch)> {
    let lookback = config.chain.max_seq_drift / config.chain.blocktime;
    let start = head_num
        .saturating_sub(lookback)
        .max(config.chain.l2_genesis.number);

    let mut refs = BTreeMap::new();
    for i in start..=head_num {
        let l2_block = provider.get_block(i.into(), true).await;
        if let Ok(Some(l2_block)) = l2_block {
            match HeadInfo::try_from_l2_block(config, l2_block) {
                Ok(head_info) => {
                    refs.insert(
                        head_info.l2_block_info.number,
                        (head_info.l2_block_info, head_info.l1_epoch),
                    );
                }
                Err(e) => {
                    tracing::warn!(err = ?e, "could not get head info for L2 block {}", i);
                }
            }
        }
    }

    refs
}
