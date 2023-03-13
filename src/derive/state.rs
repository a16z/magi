use std::{collections::BTreeMap, sync::Arc};

use ethers_core::types::H256;

use crate::{
    common::{BlockInfo, Epoch},
    config::Config,
    l1::{ChainWatcher, L1Info},
};

pub struct State {
    l1_info: BTreeMap<H256, L1Info>,
    l1_hashes: BTreeMap<u64, H256>,
    pub safe_head: BlockInfo,
    pub safe_epoch: Epoch,
    current_epoch_num: u64,
    chain_watcher: ChainWatcher,
    config: Arc<Config>,
}

impl State {
    pub fn new(
        safe_head: BlockInfo,
        safe_epoch: Epoch,
        chain_watcher: ChainWatcher,
        config: Arc<Config>,
    ) -> Self {
        Self {
            l1_info: BTreeMap::new(),
            l1_hashes: BTreeMap::new(),
            safe_head,
            safe_epoch,
            current_epoch_num: 0,
            chain_watcher,
            config,
        }
    }

    pub fn l1_info_by_hash(&self, hash: H256) -> Option<&L1Info> {
        self.l1_info.get(&hash)
    }

    pub fn l1_info_by_number(&self, num: u64) -> Option<&L1Info> {
        self.l1_hashes
            .get(&num)
            .and_then(|hash| self.l1_info.get(hash))
    }

    pub fn epoch_by_hash(&self, hash: H256) -> Option<Epoch> {
        self.l1_info_by_hash(hash).map(|info| Epoch {
            number: info.block_info.number,
            hash: info.block_info.hash,
            timestamp: info.block_info.timestamp,
        })
    }

    pub fn epoch_by_number(&self, num: u64) -> Option<Epoch> {
        self.l1_info_by_number(num).map(|info| Epoch {
            number: info.block_info.number,
            hash: info.block_info.hash,
            timestamp: info.block_info.timestamp,
        })
    }

    pub fn update_l1_info(&mut self) {
        if self.current_epoch_num > self.safe_epoch.number + 1000 {
            return;
        }

        let iter = self.chain_watcher.l1_info_receiver.try_iter();
        for l1_info in iter {
            self.current_epoch_num = l1_info.block_info.number;

            self.l1_hashes
                .insert(l1_info.block_info.number, l1_info.block_info.hash);
            self.l1_info.insert(l1_info.block_info.hash, l1_info);
        }

        self.prune();
    }

    pub fn update_safe_head(&mut self, safe_head: BlockInfo, safe_epoch: Epoch) {
        self.safe_head = safe_head;
        self.safe_epoch = safe_epoch;
    }

    fn prune(&mut self) {
        let prune_until = self.safe_epoch.number - self.config.chain.seq_window_size;
        while let Some((block_num, block_hash)) = self.l1_hashes.first_key_value() {
            if *block_num >= prune_until {
                break;
            }

            self.l1_info.remove(block_hash);
            self.l1_hashes.pop_first();
        }
    }
}
