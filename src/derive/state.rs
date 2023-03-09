use std::collections::HashMap;

use ethers_core::types::H256;

use crate::{
    common::{BlockInfo, Epoch},
    l1::{ChainWatcher, L1Info},
};

pub struct State {
    l1_info: HashMap<H256, L1Info>,
    l1_hashes: HashMap<u64, H256>,
    pub safe_head: BlockInfo,
    pub safe_epoch: Epoch,
    chain_watcher: ChainWatcher,
}

impl State {
    pub fn new(safe_head: BlockInfo, safe_epoch: Epoch, chain_watcher: ChainWatcher) -> Self {
        Self {
            l1_info: HashMap::new(),
            l1_hashes: HashMap::new(),
            safe_head,
            safe_epoch,
            chain_watcher,
        }
    }

    pub fn l1_info_by_hash(&self, hash: H256) -> Option<&L1Info> {
        let start = std::time::SystemTime::now();
        let res = self.l1_info.get(&hash);
        let finish = std::time::SystemTime::now();
        let duration = finish.duration_since(start).unwrap().as_nanos();
        tracing::info!(target: "magi", "l1 info by hash ns: {}", duration);
        res
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
        while let Ok(l1_info) = self.chain_watcher.l1_info_receiver.try_recv() {
            let start = std::time::SystemTime::now();
            self.l1_hashes
                .insert(l1_info.block_info.number, l1_info.block_info.hash);
            self.l1_info.insert(l1_info.block_info.hash, l1_info);
            let finish = std::time::SystemTime::now();
            let duration = finish.duration_since(start).unwrap().as_nanos();
            tracing::info!(target: "magi", "l1 info update ns: {}", duration);
        }
    }

    pub fn update_safe_head(&mut self, safe_head: BlockInfo, safe_epoch: Epoch) {
        self.safe_head = safe_head;
        self.safe_epoch = safe_epoch;
    }
}
