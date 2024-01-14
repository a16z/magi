use std::{collections::BTreeMap, sync::Arc};

use ethers::{
    providers::{Http, Middleware, Provider},
    types::H256,
};

use crate::{
    common::{BlockInfo, Epoch},
    config::Config,
    driver::HeadInfo,
    l1::L1Info,
};

pub struct State {
    l1_info: BTreeMap<H256, L1Info>,
    l1_hashes: BTreeMap<u64, H256>,
    l2_refs: BTreeMap<u64, (BlockInfo, Epoch)>,
    pub safe_head: BlockInfo,
    pub safe_epoch: Epoch,
    pub current_epoch_num: u64,
    config: Arc<Config>,
}

impl State {
    pub async fn new(
        finalized_head: BlockInfo,
        finalized_epoch: Epoch,
        provider: &Provider<Http>,
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

    pub fn l1_info_by_hash(&self, hash: H256) -> Option<&L1Info> {
        self.l1_info.get(&hash)
    }

    pub fn l1_info_by_number(&self, num: u64) -> Option<&L1Info> {
        self.l1_hashes
            .get(&num)
            .and_then(|hash| self.l1_info.get(hash))
    }

    pub fn l2_info_by_timestamp(&self, timestmap: u64) -> Option<&(BlockInfo, Epoch)> {
        let block_num = (timestmap - self.config.chain.l2_genesis.timestamp)
            / self.config.chain.blocktime
            + self.config.chain.l2_genesis.number;

        self.l2_refs.get(&block_num)
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

        self.update_safe_head(safe_head, safe_epoch);
    }

    pub fn update_safe_head(&mut self, safe_head: BlockInfo, safe_epoch: Epoch) {
        self.safe_head = safe_head;
        self.safe_epoch = safe_epoch;

        self.l2_refs
            .insert(self.safe_head.number, (self.safe_head, self.safe_epoch));
    }

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

async fn l2_refs(
    head_num: u64,
    provider: &Provider<Http>,
    config: &Config,
) -> BTreeMap<u64, (BlockInfo, Epoch)> {
    let lookback = config.chain.max_seq_drift / config.chain.blocktime;
    let start = head_num
        .saturating_sub(lookback)
        .max(config.chain.l2_genesis.number);

    let mut refs = BTreeMap::new();
    for i in start..=head_num {
        let block = provider.get_block_with_txs(i).await;
        if let Ok(Some(block)) = block {
            if let Ok(head_info) = HeadInfo::try_from(block) {
                refs.insert(
                    head_info.l2_block_info.number,
                    (head_info.l2_block_info, head_info.l1_epoch),
                );
            }
        }
    }

    refs
}
