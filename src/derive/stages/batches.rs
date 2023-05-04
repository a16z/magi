use core::fmt::Debug;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::io::Read;
use std::sync::{Arc, RwLock};

use ethers::types::H256;
use ethers::utils::rlp::{DecoderError, Rlp};

use eyre::Result;
use libflate::zlib::Decoder;

use crate::common::RawTransaction;
use crate::config::Config;
use crate::derive::state::State;
use crate::derive::PurgeableIterator;

use super::channels::Channel;

pub struct Batches<I> {
    /// Mapping of timestamps to batches
    batches: BTreeMap<u64, Batch>,
    channel_iter: I,
    state: Arc<RwLock<State>>,
    config: Arc<Config>,
}

impl<I> Iterator for Batches<I>
where
    I: Iterator<Item = Channel>,
{
    type Item = Batch;

    fn next(&mut self) -> Option<Self::Item> {
        self.try_next().unwrap_or_else(|_| {
            tracing::debug!("Failed to decode batch");
            None
        })
    }
}

impl<I> PurgeableIterator for Batches<I>
where
    I: PurgeableIterator<Item = Channel>,
{
    fn purge(&mut self) {
        self.channel_iter.purge();
        self.batches.clear();
    }
}

impl<I> Batches<I> {
    pub fn new(channel_iter: I, state: Arc<RwLock<State>>, config: Arc<Config>) -> Self {
        Self {
            batches: BTreeMap::new(),
            channel_iter,
            state,
            config,
        }
    }
}

impl<I> Batches<I>
where
    I: Iterator<Item = Channel>,
{
    fn try_next(&mut self) -> Result<Option<Batch>> {
        let channel = self.channel_iter.next();
        if let Some(channel) = channel {
            let batches = decode_batches(&channel)?;
            batches.into_iter().for_each(|batch| {
                self.batches.insert(batch.timestamp, batch);
            });
        }

        let derived_batch = loop {
            if let Some((_, batch)) = self.batches.first_key_value() {
                match self.batch_status(batch) {
                    BatchStatus::Accept => {
                        let batch = batch.clone();
                        self.batches.remove(&batch.timestamp);
                        break Some(batch);
                    }
                    BatchStatus::Drop => {
                        tracing::warn!("dropping invalid batch");
                        let timestamp = batch.timestamp;
                        self.batches.remove(&timestamp);
                    }
                    BatchStatus::Future | BatchStatus::Undecided => {
                        break None;
                    }
                }
            } else {
                break None;
            }
        };

        let batch = if derived_batch.is_none() {
            let state = self.state.read().unwrap();

            let current_l1_block = state.current_epoch_num;
            let safe_head = state.safe_head;
            let epoch = state.safe_epoch;
            let next_epoch = state.epoch_by_number(epoch.number + 1);
            let seq_window_size = self.config.chain.seq_window_size;

            if let Some(next_epoch) = next_epoch {
                if current_l1_block > epoch.number + seq_window_size {
                    let next_timestamp = safe_head.timestamp + self.config.chain.blocktime;
                    let epoch = if next_timestamp < next_epoch.timestamp {
                        epoch
                    } else {
                        next_epoch
                    };

                    Some(Batch {
                        epoch_num: epoch.number,
                        epoch_hash: epoch.hash,
                        parent_hash: safe_head.parent_hash,
                        timestamp: next_timestamp,
                        transactions: Vec::new(),
                        l1_inclusion_block: current_l1_block,
                    })
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            derived_batch
        };

        Ok(batch)
    }

    fn batch_status(&self, batch: &Batch) -> BatchStatus {
        let state = self.state.read().unwrap();
        let epoch = state.safe_epoch;
        let next_epoch = state.epoch_by_number(epoch.number + 1);
        let head = state.safe_head;
        let next_timestamp = head.timestamp + self.config.chain.blocktime;

        // check timestamp range
        match batch.timestamp.cmp(&next_timestamp) {
            Ordering::Greater => return BatchStatus::Future,
            Ordering::Less => return BatchStatus::Drop,
            Ordering::Equal => (),
        }

        // check that block builds on existing chain
        if batch.parent_hash != head.hash {
            tracing::warn!("invalid parent hash");
            return BatchStatus::Drop;
        }

        // check the inclusion delay
        if batch.epoch_num + self.config.chain.seq_window_size < batch.l1_inclusion_block {
            tracing::warn!("inclusion window elapsed");
            return BatchStatus::Drop;
        }

        // check and set batch origin epoch
        let batch_origin = if batch.epoch_num == epoch.number {
            Some(epoch)
        } else if batch.epoch_num == epoch.number + 1 {
            next_epoch
        } else {
            tracing::warn!("invalid batch origin epoch number");
            return BatchStatus::Drop;
        };

        if let Some(batch_origin) = batch_origin {
            if batch.epoch_hash != batch_origin.hash {
                tracing::warn!("invalid epoch hash");
                return BatchStatus::Drop;
            }

            if batch.timestamp < batch_origin.timestamp {
                tracing::warn!("batch too old");
                return BatchStatus::Drop;
            }

            // handle sequencer drift
            if batch.timestamp > batch_origin.timestamp + self.config.chain.max_seq_drift {
                if batch.transactions.is_empty() {
                    if epoch.number == batch.epoch_num {
                        if let Some(next_epoch) = next_epoch {
                            if batch.timestamp >= next_epoch.timestamp {
                                tracing::warn!("sequencer drift too large");
                                return BatchStatus::Drop;
                            }
                        } else {
                            return BatchStatus::Undecided;
                        }
                    }
                } else {
                    tracing::warn!("sequencer drift too large");
                    return BatchStatus::Drop;
                }
            }
        } else {
            return BatchStatus::Undecided;
        }

        if batch.has_invalid_transactions() {
            tracing::warn!("invalid transaction");
            return BatchStatus::Drop;
        }

        BatchStatus::Accept
    }
}

fn decode_batches(channel: &Channel) -> Result<Vec<Batch>> {
    let mut channel_data = Vec::new();
    let mut d = Decoder::new(channel.data.as_slice())?;
    d.read_to_end(&mut channel_data)?;

    let mut batches = Vec::new();
    let mut offset = 0;

    while offset < channel_data.len() {
        let batch_rlp = Rlp::new(&channel_data[offset..]);
        let batch_info = batch_rlp.payload_info()?;

        let batch_data: Vec<u8> = batch_rlp.as_val()?;

        let batch_content = &batch_data[1..];
        let rlp = Rlp::new(batch_content);
        let size = rlp.payload_info()?.total();

        let batch = Batch::decode(&rlp, channel.l1_inclusion_block)?;
        batches.push(batch);

        offset += size + batch_info.header_len + 1;
    }

    Ok(batches)
}

#[derive(Debug, Clone)]
pub struct Batch {
    pub parent_hash: H256,
    pub epoch_num: u64,
    pub epoch_hash: H256,
    pub timestamp: u64,
    pub transactions: Vec<RawTransaction>,
    pub l1_inclusion_block: u64,
}

#[derive(Debug, Clone, PartialEq)]
enum BatchStatus {
    Drop,
    Accept,
    Undecided,
    Future,
}

impl Batch {
    fn decode(rlp: &Rlp, l1_inclusion_block: u64) -> Result<Self, DecoderError> {
        let parent_hash = rlp.val_at(0)?;
        let epoch_num = rlp.val_at(1)?;
        let epoch_hash = rlp.val_at(2)?;
        let timestamp = rlp.val_at(3)?;
        let transactions = rlp.list_at(4)?;

        Ok(Batch {
            parent_hash,
            epoch_num,
            epoch_hash,
            timestamp,
            transactions,
            l1_inclusion_block,
        })
    }

    fn has_invalid_transactions(&self) -> bool {
        self.transactions
            .iter()
            .any(|tx| tx.0.is_empty() || tx.0[0] == 0x7E)
    }
}

#[cfg(test)]
mod tests {
    use crate::common::{BlockInfo, Epoch};
    use crate::config::{ChainConfig, Config};
    use crate::derive::stages::batcher_transactions::{
        BatcherTransactionMessage, BatcherTransactions,
    };
    use crate::derive::stages::batches::{decode_batches, Batches};
    use crate::derive::stages::channels::{Channel, Channels};
    use crate::derive::state::State;
    use ethers::types::H256;
    use std::str::FromStr;
    use std::sync::{mpsc, Arc, RwLock};

    #[test]
    fn test_decode_batches() {
        let data = "78dad459793894fbdb7f9e19db20eb902dbb9086b410b2af2939b66c255bd60991c8a133c6845276c9daa36c21bb3211932c8908591a6509132a3b1959decbe93ade73aeebbc745ee7f773755fd7fcf599effdfd3cf7f5b93ff7f7f93e786a5d804cad05255ef05f6445189cc97f1b4ef3656d2cdd318bcbe30a93f689737dea1f3297ed8d83029fa64364f70716e8c138e517e81606f661f754e982039eda1366dc277286510cf7142b717613166832d56279316cb1000ba65305f1e230eb3fec23da58628342a55fc9ee47fa1db79e1d672c3968bfd4740253ae81b0ca2a01fe1456ad32f374dd47270af5fcc69839881590a92137b059305c9d2280500faf1a489d7179f26143eb2923944efb05a1381b4536499f9ed9be14ff2817142427de6d4a59af3be62c8fa3d5927fef3615e6226f4bc1ad412d4b8c609853dc8b87b591612d4170a5d9df4953a7f1c73ebc397a8f742d3526ac08559a86953e948c9e75c7e061f68d186f3960f3c06c0e83d0e6380c0041601bf197c591f9a7553e1647f6f171fa191419c90d03f08605100061f06d6c60bd054eb119788b6b8ff14ee2eb052e0af978632db54e63fed6900a3ad0b179456da86a97b9134d00b9d0b04b97a604dd743bb92fa035f0412bec13a2793e7a9ad5d33bb1bdcbf20d22146377f9d0ca56f9d51733a63507dc9270cc575fd67821d24e1d76a18bce5c503c7105ed33cd51c62075c2284ee2e2120bf1154d553ccc2694c37ef478185d64e7c7e23d8d1ca784c7b17034d436d228729fd385b9a73a2900b0adc7ec9ebe6a12bbd61c2b23cc5ab27a0bd80beda6203f2ef8e02540f41dd4154ca8b52563434b3a0d6dae239607cff261e9f4cbf317f3b030b72030180a02cf45c6d6f5b401fb6e5f1ae6541b1a1fafe55ab9b462e28729d77840995cf167f2bd365a1af9538a93022353d6019218be002b7cfba60fbb348559e7cb9ca6cc20642cf82997cb7d58b7c2c919b96f29f9f0c52ceb792c4ec403adcf025d38461918536ade57d6256794c54d9591726b85ae5ca645790264f5ce99be48fcce9766836f76e9b73c52a9fd2c2a675e4122f85d148b406cd3f6f8c2ca860ad88b4201609def590ffbe3d8667b8495284986b19e918fd4f26e7aacf5e8d7bc6733e3bda1f65a90a4b901166e8317198816e8b8f6a235b2735954b95a877177b165b1dd19064d9eef7cb936f83a68a52447c996a14e2d7967b2a0f20a8e440bc8fc8bf54da41df6d00a95ee76eea6a1e43cd90b374dee48a889b33ec87480a8d776204b17e24aa9f787efc9cb246560634d57bf1ac252549f9d9f9f4b141f0ba3435c09837fe71bb8c1f7ffb0e4edf20518d554d6f97211849d7bdf9e1d4d6dad75f3ffaa29f5f5bed74c291159ddb4d274dd4c7f72113a2f9fe17534fc9b46f02ffcb153d6a0efcd41c7de92d78f16e73cbeec5b2496f17fe71bbcc1175fd6914a7890e046782b25d58a0e33c8e046996e932f68a7e97bf6c6773dd414db0992ee66f862efd7b0d4cbb38a2725a6b15af899c579f5f73395a46ac6439a19c1ac17300a69dd16434ea3f0abc7382c254daeedb28cb28ce8a4715a16f3c0532e0164ca052880911a317f464a05ac6f507f15e4d2507c37acc2672f2a65ba89452cd462e4c10f0f53373265f61f83c987716330c5ad883c130aef10d535124188963915286248c63fe160a25aa04ce01182bdcf7cabffe445c9c402006fa1d9c9c12406bec7637610ffbdc0114419d3d1c2665984e38779b84aa0406a349297e54ec1a783c92edc841c4a5f8af3ab9fa54b24fb31dfd02339b55153b01c472a83b7bf729c6ea4d16268a519df2abfc77da516e51cbad5b523bacf2fa0510ca7809952648a79ee1749ae815455db8bbf5adc99f5ca08a2486c653e8ab649921b701814ef71ed1c312261efe82c0c7960e1aed0ac772a7a2d4a8ad5c72cfe4b4153af34aa62f09866423392fe1ee9158054e7877883c2be453f6f873fbcc5bfa785cf96646d7020bba6b16726f7bd76bf8e6b9ec886a69936346d9eef031cbddfef860b9aa276fc98d9e57b7282f0dfd2f4f6e22f9adecf6ec5acb74cef4d49beeedc4b607f0cc01b0c7750d3300d5ea95f13770efffea7ee9214aa608830831027a6cac7e43f5263b609ec5ac8392856353d8d543ca1f56c7fa91581533ba051a7521ea8b3406775e144c3f49fa69ee7c4b19d344a99df2abfad67aa357a685e092af3f27baa103215d1299e79bcdf523975e98d79bc8892bf67f091e78d11d8525ac973c7925330ef4a1f45f7e851fa464c16e2bc6fb8ea74ad9bbf6cad30116d6eef0e98654be15e71c33a9d6a54709f9cd192375a7b68ba8509905f524396ac59cb99b80757cbd2ae33093dbd51d426ee10ec98b966fde1e81919bb727d60f12444e546317fcd852c9fa41a622735d32f28716c9a7726dcedf3613a7782a67888c40f5bbf07e18f69a29975d88f645a878b8f9889ef2f9c2f2aa6d5e7111be9e71825db4ebef6375bf9e1949e7f9a264a731b9d57aa9d548c58ae610dcc797a805e9e0920b0d405ff849d3737009e8af53f45acfddc95f16a36c40c80bfe6ded1d71c9670466827f1f502fb36485df66b7c3d35669fdb34dd9ed97fd3d78a973eb0c1c4452f212660cc155545bc93f3755f150a56e0453410f37a721e465d48f09b5f26a97356cac9cb176f957f8f0ca7d01518275b5c9cf7a3eb7908dc9bc84ee704915bb4353aba2bc01d9b2277fc527487470d429f45f8dd2ac154d9a24af8c85be039e5a0125f95414f1b6ebdf3507abe4371059ecb17564fe60829d393a4af4dc91ba02869451ba5579a726f8f43f23315d143b465b436cbd5c65c2c7eec76e99ae3d1e6c885f7b9b56d079db9fff7d57d7e43d346056b4b3e80fd41a4ab83bfe3924fd91bca2b0a3fe1098961d9770959672e55d1203cce4573c60180d7b351eda4a62588777c77125f2f3045fa5304178bfee869bb89570f6119d16abb5e8f7334266864d5791cacd655e1ad9b2b9cd60aebb5d2b538322818315e3bd9fd793f4cea6925ca7c363d2d245170abfcdad50d221509fa89e7083c4f92436dbe527a7f48fdd6c24edb36991e8874e83cab0406a0463b966ff376f194e14c4171a5b05d3cfb4cd69e0512e063ed87e32faf9f900afd761f9e7858d96fc600e3e353e7bae4d0dbe455f6f5b9e31beef4625537273988514d2088e8d79c14162c29955b91ef33a8467208283ffdd0750fcbeebd6c621578582e408665419705c9a3495ac8b9ea9595986cf5cc03579bd43d898e96c55cc5828691b5f8ea1f36ff4b6498391e761a46861962c1f4200a5c355694092bca1404fa88c536b029cbce2c0d1cfb86465a4a08ed0ebe7badc715830787d113aec15b946b8b7600f9b7c0adb7d76effac9ffe26b6e007506b1aeb48991869fca7f6a7d9c67ad1b9884307b6b93f4800a1eceb15cb4e3ebc394e77da220de3b227739a05094f3e4848d3199b2255ba431ca0dfa8f5625fba3725f9d3c514c5513c763b7caffbfaa43a77411e876ac8b94fbc56788a11804c31089994cc79d273068924c7ef9f5de11a4ea6da0f321316f7cf7774f5843712448c7e58ad97c914311bb6beb061eb6946166e1c98bdef8e2c921e63a4ed085d0db4693fa1addb84a7db0f7649c488528df6a9f1be1c05e0a37d7010beade3d0b66c1d085966df161e8adafcc6355496632bdbcd825623f88f18b7f1b9c2cfa949bf793859c51a57a8c23cbc7f7af5aa5155f1dcf1c71de23c0bfcb40a09aa4deda6050c8569ab2f5c537eb9e087c42c3a670c286e959f5fcf1e57393465caf598def15e14c588dd70884248da9c6b6bd44d54cc73bde72a23aa259d7b8ff77d8ae97b3150e021245ddf4ada65661daf806e9d9dabec5558b7f550ebf7ec260b16b6eeca8b7a1aaaf9c5a26c0d951e22723402ab211f1e29dba840729edee9496582beaad4554e5e2eed3d11a14283c9e23ace5d2b4e433d0fcc3078b0124606cbb1603aec8f6f23415408e358da0a8b733edac893e8b77bef4f59328a6ae5d3ca87b0e58e7f115001f0a0c6214938f69fb4f9df5d94fd7349511c8be8f76872e109bd9bc6c2fdfff03993e49ed485a226b1da209b4d975acc32f9a900ffa6cfffddf31340280d2efa59844d59a7ec592dd5a87998b6113506c44c665ca197cebff1c90e5484cc8a6cb2c5b1badab35aefa35c1384f0bb6459061ad574c2f37f8bbbd2e8dff5f27f020000ffff8db46838";

        let channel = Channel {
            id: 0,
            data: hex::decode(data).unwrap(),
            l1_inclusion_block: 0,
        };
        let batches = decode_batches(&channel).unwrap();
        assert_eq!(batches.len(), 6);
        assert_eq!(
            batches[0].parent_hash,
            H256::from_str("0x9a6d7cf81309515caed98e08edffe9a467a71a707910474b5fde43e2a6fc6454")
                .unwrap()
        );
        assert_eq!(batches[0].transactions[0].0, hex::decode("02f8af8201a406831e8480831e848082b60a94dac73bbc7ab317b64fd38dc1490fb33264facb4b80b844095ea7b30000000000000000000000005ecf36f50bd738eeb04d7ae91118dabba057521b00000000000000000000000000000000000000000000000000000000000e7ef0c001a0d81e420107a576efadc7cf4e532d48d26d2395613d152e77d8f3de9d7c916a2da03929d75705c64ae9693a840a463add6ef08b6a3a62041b440737da8f0ef43d26").unwrap());
        assert_eq!(batches[0].transactions.len(), 7);
        assert_eq!(batches[5].epoch_num, 8502692);
        assert_eq!(batches[5].timestamp, 1676565110);
        assert!(!batches[2].has_invalid_transactions());
    }

    #[test]
    fn test_try_next() {
        let data = "00b3ec7df691dc58384222fbdc05891b08000000000bd478dad459793894fbdb7f9e19db20eb902dbb9086b410b2af2939b66c255bd60991c8a133c6845276c9daa36c21bb3211932c8908591a6509132a3b1959decbe93ade73aeebbc745ee7f773755fd7fcf599effdfd3cf7f5b93ff7f7f93e786a5d804cad05255ef05f6445189cc97f1b4ef3656d2cdd318bcbe30a93f689737dea1f3297ed8d83029fa64364f70716e8c138e517e81606f661f754e982039eda1366dc277286510cf7142b717613166832d56279316cb1000ba65305f1e230eb3fec23da58628342a55fc9ee47fa1db79e1d672c3968bfd4740253ae81b0ca2a01fe1456ad32f374dd47270af5fcc69839881590a92137b059305c9d2280500faf1a489d7179f26143eb2923944efb05a1381b4536499f9ed9be14ff2817142427de6d4a59af3be62c8fa3d5927fef3615e6226f4bc1ad412d4b8c609853dc8b87b591612d4170a5d9df4953a7f1c73ebc397a8f742d3526ac08559a86953e948c9e75c7e061f68d186f3960f3c06c0e83d0e6380c0041601bf197c591f9a7553e1647f6f171fa191419c90d03f08605100061f06d6c60bd054eb119788b6b8ff14ee2eb052e0af978632db54e63fed6900a3ad0b179456da86a97b9134d00b9d0b04b97a604dd743bb92fa035f0412bec13a2793e7a9ad5d33bb1bdcbf20d22146377f9d0ca56f9d51733a63507dc9270cc575fd67821d24e1d76a18bce5c503c7105ed33cd51c62075c2284ee2e2120bf1154d553ccc2694c37ef478185d64e7c7e23d8d1ca784c7b17034d436d228729fd385b9a73a2900b0adc7ec9ebe6a12bbd61c2b23cc5ab27a0bd80beda6203f2ef8e02540f41dd4154ca8b52563434b3a0d6dae239607cff261e9f4cbf317f3b030b72030180a02cf45c6d6f5b401fb6e5f1ae6541b1a1fafe55ab9b462e28729d77840995cf167f2bd365a1af9538a93022353d6019218be002b7cfba60fbb348559e7cb9ca6cc20642cf82997cb7d58b7c2c919b96f29f9f0c52ceb792c4ec403adcf025d38461918536ade57d6256794c54d9591726b85ae5ca645790264f5ce99be48fcce9766836f76e9b73c52a9fd2c2a675e4122f85d148b406cd3f6f8c2ca860ad88b4201609def590ffbe3d8667b8495284986b19e918fd4f26e7aacf5e8d7bc6733e3bda1f65a90a4b901166e8317198816e8b8f6a235b2735954b95a877177b165b1dd19064d9eef7cb936f83a68a52447c996a14e2d7967b2a0f20a8e440bc8fc8bf54da41df6d00a95ee76eea6a1e43cd90b374dee48a889b33ec87480a8d776204b17e24aa9f787efc9cb246560634d57bf1ac252549f9d9f9f4b141f0ba3435c09837fe71bb8c1f7ffb0e4edf20518d554d6f97211849d7bdf9e1d4d6dad75f3ffaa29f5f5bed74c291159ddb4d274dd4c7f72113a2f9fe17534fc9b46f02ffcb153d6a0efcd41c7de92d78f16e73cbeec5b2496f17fe71bbcc1175fd6914a7890e046782b25d58a0e33c8e046996e932f68a7e97bf6c6773dd414db0992ee66f862efd7b0d4cbb38a2725a6b15af899c579f5f73395a46ac6439a19c1ac17300a69dd16434ea3f0abc7382c254daeedb28cb28ce8a4715a16f3c0532e0164ca052880911a317f464a05ac6f507f15e4d2507c37acc2672f2a65ba89452cd462e4c10f0f53373265f61f83c987716330c5ad883c130aef10d535124188963915286248c63fe160a25aa04ce01182bdcf7cabffe445c9c402006fa1d9c9c12406bec7637610ffbdc0114419d3d1c2665984e38779b84aa0406a349297e54ec1a783c92edc841c4a5f8af3ab9fa54b24fb31dfd02339b55153b01c472a83b7bf729c6ea4d16268a519df2abfc77da516e51cbad5b523bacf2fa0510ca7809952648a79ee1749ae815455db8bbf5adc99f5ca08a2486c653e8ab649921b701814ef71ed1c312261efe82c0c7960e1aed0ac772a7a2d4a8ad5c72cfe4b4153af34aa62f09866423392fe1ee9158054e7877883c2be453f6f873fbcc5bfa785cf96646d7020bba6b16726f7bd76bf8e6b9ec886a69936346d9eef031cbddfef860b9aa276fc98d9e57b7282f0dfd2f4f6e22f9adecf6ec5acb74cef4d49beeedc4b607f0cc01b0c7750d3300d5ea95f13770efffea7ee9214aa608830831027a6cac7e43f5263b609ec5ac8392856353d8d543ca1f56c7fa91581533ba051a7521ea8b3406775e144c3f49fa69ee7c4b19d344a99df2abfad67aa357a685e092af3f27baa103215d1299e79bcdf523975e98d79bc8892bf67f091e78d11d8525ac973c7925330ef4a1f45f7e851fa464c16e2bc6fb8ea74ad9bbf6cad30116d6eef0e98654be15e71c33a9d6a54709f9cd192375a7b68ba8509905f524396ac59cb99b80757cbd2ae33093dbd51d426ee10ec98b966fde1e81919bb727d60f12444e546317fcd852c9fa41a622735d32f28716c9a7726dcedf3613a7782a67888c40f5bbf07e18f69a29975d88f645a878b8f9889ef2f9c2f2aa6d5e7111be9e71825db4ebef6375bf9e1949e7f9a264a731b9d57aa9d548c58ae610dcc797a805e9e0920b0d405ff849d3737009e8af53f45acfddc95f16a36c40c80bfe6ded1d71c9670466827f1f502fb36485df66b7c3d35669fdb34dd9ed97fd3d78a973eb0c1c4452f212660cc155545bc93f3755f150a56e0453410f37a721e465d48f09b5f26a97356cac9cb176f957f8f0ca7d01518275b5c9cf7a3eb7908dc9bc84ee704915bb4353aba2bc01d9b2277fc527487470d429f45f8dd2ac154d9a24af8c85be039e5a0125f95414f1b6ebdf3507abe4371059ecb17564fe60829d393a4af4dc91ba02869451ba5579a726f8f43f23315d143b465b436cbd5c65c2c7eec76e99ae3d1e6c885f7b9b56d079db9fff7d57d7e43d346056b4b3e80fd41a4ab83bfe3924fd91bca2b0a3fe1098961d9770959672e55d1203cce4573c60180d7b351eda4a62588777c77125f2f3045fa5304178bfee869bb89570f6119d16abb5e8f7334266864d5791cacd655e1ad9b2b9cd60aebb5d2b538322818315e3bd9fd793f4cea6925ca7c363d2d245170abfcdad50d221509fa89e7083c4f92436dbe527a7f48fdd6c24edb36991e8874e83cab0406a0463b966ff376f194e14c4171a5b05d3cfb4cd69e0512e063ed87e32faf9f900afd761f9e7858d96fc600e3e353e7bae4d0dbe455f6f5b9e31beef4625537273988514d2088e8d79c14162c29955b91ef33a8467208283ffdd0750fcbeebd6c621578582e408665419705c9a3495ac8b9ea9595986cf5cc03579bd43d898e96c55cc5828691b5f8ea1f36ff4b6498391e761a46861962c1f4200a5c355694092bca1404fa88c536b029cbce2c0d1cfb86465a4a08ed0ebe7badc715830787d113aec15b946b8b7600f9b7c0adb7d76effac9ffe26b6e007506b1aeb48991869fca7f6a7d9c67ad1b9884307b6b93f4800a1eceb15cb4e3ebc394e77da220de3b227739a05094f3e4848d3199b2255ba431ca0dfa8f5625fba3725f9d3c514c5513c763b7caffbfaa43a77411e876ac8b94fbc56788a11804c31089994cc79d273068924c7ef9f5de11a4ea6da0f321316f7cf7774f5843712448c7e58ad97c914311bb6beb061eb6946166e1c98bdef8e2c921e63a4ed085d0db4693fa1addb84a7db0f7649c488528df6a9f1be1c05e0a37d7010beade3d0b66c1d085966df161e8adafcc6355496632bdbcd825623f88f18b7f1b9c2cfa949bf793859c51a57a8c23cbc7f7af5aa5155f1dcf1c71de23c0bfcb40a09aa4deda6050c8569ab2f5c537eb9e087c42c3a670c286e959f5fcf1e57393465caf598def15e14c588dd70884248da9c6b6bd44d54cc73bde72a23aa259d7b8ff77d8ae97b3150e021245ddf4ada65661daf806e9d9dabec5558b7f550ebf7ec260b16b6eeca8b7a1aaaf9c5a26c0d951e22723402ab211f1e29dba840729edee9496582beaad4554e5e2eed3d11a14283c9e23ace5d2b4e433d0fcc3078b0124606cbb1603aec8f6f23415408e358da0a8b733edac893e8b77bef4f59328a6ae5d3ca87b0e58e7f115001f0a0c6214938f69fb4f9df5d94fd7349511c8be8f76872e109bd9bc6c2fdfff03993e49ed485a226b1da209b4d975acc32f9a900ffa6cfffddf31340280d2efa59844d59a7ec592dd5a87998b6113506c44c665ca197cebff1c90e5484cc8a6cb2c5b1badab35aefa35c1384f0bb6459061ad574c2f37f8bbbd2e8dff5f27f020000ffff8db4683801";

        let data = hex::decode(data).unwrap();
        let txs = vec![data.clone()];
        let (tx, rx) = mpsc::channel();
        let batch_transactions = BatcherTransactions::new(rx);

        let res = tx.send(BatcherTransactionMessage {
            txs,
            l1_origin: 123456,
        });
        assert!(res.is_ok());

        let config = Config {
            l1_rpc_url: "".to_string(),
            l2_rpc_url: "".to_string(),
            l2_engine_url: "".to_string(),
            chain: ChainConfig::base_goerli(),
            jwt_secret: "".to_string(),
            rpc_port: 0,
        };
        let channels = Channels::new(batch_transactions, Arc::new(config.clone()));

        let state = State::new(
            BlockInfo {
                hash: Default::default(),
                number: 0,
                parent_hash: Default::default(),
                timestamp: 0,
            },
            Epoch {
                number: 0,
                hash: Default::default(),
                timestamp: 0,
            },
            Arc::new(config.clone()),
        );
        let mut batches = Batches::new(
            channels,
            Arc::new(RwLock::new(state)),
            Arc::new(config.clone()),
        );
        assert!(batches.next().is_none());
    }
}
