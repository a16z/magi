use std::collections::BTreeMap;

use ethers::types::H256;

use crate::{
    l1::L1Info,
    types::common::{BlockInfo, Epoch},
};

pub struct State {
    l1_info: BTreeMap<H256, L1Info>,
    l1_hashes: BTreeMap<u64, H256>,
    pub safe_head: BlockInfo,
    pub safe_epoch: Epoch,
    pub unsafe_head: BlockInfo,
    pub unsafe_epoch: Epoch,
    pub current_epoch_num: u64,
    seq_window_size: u64,
}

impl State {
    pub fn new(
        finalized_head: BlockInfo,
        finalized_epoch: Epoch,
        unsafe_head: BlockInfo,
        unsafe_epoch: Epoch,
        seq_window_size: u64,
    ) -> Self {
        Self {
            l1_info: BTreeMap::new(),
            l1_hashes: BTreeMap::new(),
            safe_head: finalized_head,
            safe_epoch: finalized_epoch,
            unsafe_head,
            unsafe_epoch,
            current_epoch_num: 0,
            seq_window_size,
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

    pub fn l1_info_current(&self) -> Option<&L1Info> {
        self.l1_hashes
            .get(&self.current_epoch_num)
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

    pub fn update_l1_info(&mut self, l1_info: L1Info) {
        self.current_epoch_num = l1_info.block_info.number;

        self.l1_hashes
            .insert(l1_info.block_info.number, l1_info.block_info.hash);
        self.l1_info.insert(l1_info.block_info.hash, l1_info);

        self.prune();
    }

    pub fn purge(&mut self, safe_head: BlockInfo, safe_epoch: Epoch) {
        self.current_epoch_num = 0;
        self.l1_info.clear();
        self.l1_hashes.clear();

        self.safe_head = safe_head;
        self.safe_epoch = safe_epoch;

        self.unsafe_head = safe_head;
        self.unsafe_epoch = safe_epoch;
    }

    pub fn update_safe_head(&mut self, safe_head: BlockInfo, safe_epoch: Epoch) {
        self.safe_head = safe_head;
        self.safe_epoch = safe_epoch;

        if self.safe_head.number > self.unsafe_head.number {
            self.unsafe_head = safe_head;
            self.unsafe_epoch = safe_epoch;
        }
    }

    pub fn update_unsafe_head(&mut self, unsafe_head: BlockInfo, unsafe_epoch: Epoch) {
        self.unsafe_head = unsafe_head;
        self.unsafe_epoch = unsafe_epoch;
    }

    fn prune(&mut self) {
        let prune_until = self.safe_epoch.number.saturating_sub(self.seq_window_size);

        while let Some((block_num, block_hash)) = self.l1_hashes.first_key_value() {
            if *block_num >= prune_until {
                break;
            }

            self.l1_info.remove(block_hash);
            self.l1_hashes.pop_first();
        }
    }
}
