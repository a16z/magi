use std::sync::mpsc;

use eyre::Result;
use std::collections::VecDeque;

use crate::derive::PurgeableIterator;

pub struct BatcherTransactionMessage {
    pub txs: Vec<Vec<u8>>,
    pub l1_origin: u64,
}

pub struct BatcherTransactions {
    txs: VecDeque<BatcherTransaction>,
    transaction_rx: mpsc::Receiver<BatcherTransactionMessage>,
}

impl Iterator for BatcherTransactions {
    type Item = BatcherTransaction;

    fn next(&mut self) -> Option<Self::Item> {
        self.process_incoming();
        self.txs.pop_front()
    }
}

impl PurgeableIterator for BatcherTransactions {
    fn purge(&mut self) {
        // drain the channel first
        while let Ok(_) = self.transaction_rx.try_recv() {}
        self.txs.clear();
    }
}

impl BatcherTransactions {
    pub fn new(transaction_rx: mpsc::Receiver<BatcherTransactionMessage>) -> Self {
        Self {
            transaction_rx,
            txs: VecDeque::new(),
        }
    }

    pub fn process_incoming(&mut self) {
        while let Ok(BatcherTransactionMessage { txs, l1_origin }) = self.transaction_rx.try_recv()
        {
            for data in txs {
                let res = BatcherTransaction::new(&data, l1_origin).map(|tx| {
                    self.txs.push_back(tx);
                });

                if res.is_err() {
                    tracing::warn!("dropping invalid batcher transaction");
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct BatcherTransaction {
    pub version: u8,
    pub frames: Vec<Frame>,
}

impl BatcherTransaction {
    pub fn new(data: &[u8], l1_origin: u64) -> Result<Self> {
        let version = data[0];
        let frame_data = data.get(1..).ok_or(eyre::eyre!("No frame data"))?;

        let mut offset = 0;
        let mut frames = Vec::new();
        while offset < frame_data.len() {
            let (frame, next_offset) = Frame::from_data(frame_data, offset, l1_origin)?;
            frames.push(frame);
            offset = next_offset;
        }

        Ok(Self { version, frames })
    }
}

#[derive(Debug, Default, Clone)]
pub struct Frame {
    pub channel_id: u128,
    pub frame_number: u16,
    pub frame_data_len: u32,
    pub frame_data: Vec<u8>,
    pub is_last: bool,
    pub l1_inclusion_block: u64,
}

impl Frame {
    fn from_data(data: &[u8], offset: usize, l1_inclusion_block: u64) -> Result<(Self, usize)> {
        let data = &data[offset..];

        if data.len() < 23 {
            eyre::bail!("invalid frame size");
        }

        let channel_id = u128::from_be_bytes(data[0..16].try_into()?);
        let frame_number = u16::from_be_bytes(data[16..18].try_into()?);
        let frame_data_len = u32::from_be_bytes(data[18..22].try_into()?);

        let frame_data_end = 22 + frame_data_len as usize;
        if data.len() < frame_data_end {
            eyre::bail!("invalid frame size");
        }

        let frame_data = data[22..frame_data_end].to_vec();

        let is_last = data[frame_data_end] != 0;

        let frame = Self {
            channel_id,
            frame_number,
            frame_data_len,
            frame_data,
            is_last,
            l1_inclusion_block,
        };

        Ok((frame, offset + data.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TX_DATA: &str = "00b3ec7df691dc58384222fbdc05891b08000000000bd478dad459793894fbdb7f9e19db20eb902dbb9086b410b2af2939b66c255bd60991c8a133c6845276c9daa36c21bb3211932c8908591a6509132a3b1959decbe93ade73aeebbc745ee7f773755fd7fcf599effdfd3cf7f5b93ff7f7f93e786a5d804cad05255ef05f6445189cc97f1b4ef3656d2cdd318bcbe30a93f689737dea1f3297ed8d83029fa64364f70716e8c138e517e81606f661f754e982039eda1366dc277286510cf7142b717613166832d56279316cb1000ba65305f1e230eb3fec23da58628342a55fc9ee47fa1db79e1d672c3968bfd4740253ae81b0ca2a01fe1456ad32f374dd47270af5fcc69839881590a92137b059305c9d2280500faf1a489d7179f26143eb2923944efb05a1381b4536499f9ed9be14ff2817142427de6d4a59af3be62c8fa3d5927fef3615e6226f4bc1ad412d4b8c609853dc8b87b591612d4170a5d9df4953a7f1c73ebc397a8f742d3526ac08559a86953e948c9e75c7e061f68d186f3960f3c06c0e83d0e6380c0041601bf197c591f9a7553e1647f6f171fa191419c90d03f08605100061f06d6c60bd054eb119788b6b8ff14ee2eb052e0af978632db54e63fed6900a3ad0b179456da86a97b9134d00b9d0b04b97a604dd743bb92fa035f0412bec13a2793e7a9ad5d33bb1bdcbf20d22146377f9d0ca56f9d51733a63507dc9270cc575fd67821d24e1d76a18bce5c503c7105ed33cd51c62075c2284ee2e2120bf1154d553ccc2694c37ef478185d64e7c7e23d8d1ca784c7b17034d436d228729fd385b9a73a2900b0adc7ec9ebe6a12bbd61c2b23cc5ab27a0bd80beda6203f2ef8e02540f41dd4154ca8b52563434b3a0d6dae239607cff261e9f4cbf317f3b030b72030180a02cf45c6d6f5b401fb6e5f1ae6541b1a1fafe55ab9b462e28729d77840995cf167f2bd365a1af9538a93022353d6019218be002b7cfba60fbb348559e7cb9ca6cc20642cf82997cb7d58b7c2c919b96f29f9f0c52ceb792c4ec403adcf025d38461918536ade57d6256794c54d9591726b85ae5ca645790264f5ce99be48fcce9766836f76e9b73c52a9fd2c2a675e4122f85d148b406cd3f6f8c2ca860ad88b4201609def590ffbe3d8667b8495284986b19e918fd4f26e7aacf5e8d7bc6733e3bda1f65a90a4b901166e8317198816e8b8f6a235b2735954b95a877177b165b1dd19064d9eef7cb936f83a68a52447c996a14e2d7967b2a0f20a8e440bc8fc8bf54da41df6d00a95ee76eea6a1e43cd90b374dee48a889b33ec87480a8d776204b17e24aa9f787efc9cb246560634d57bf1ac252549f9d9f9f4b141f0ba3435c09837fe71bb8c1f7ffb0e4edf20518d554d6f97211849d7bdf9e1d4d6dad75f3ffaa29f5f5bed74c291159ddb4d274dd4c7f72113a2f9fe17534fc9b46f02ffcb153d6a0efcd41c7de92d78f16e73cbeec5b2496f17fe71bbcc1175fd6914a7890e046782b25d58a0e33c8e046996e932f68a7e97bf6c6773dd414db0992ee66f862efd7b0d4cbb38a2725a6b15af899c579f5f73395a46ac6439a19c1ac17300a69dd16434ea3f0abc7382c254daeedb28cb28ce8a4715a16f3c0532e0164ca052880911a317f464a05ac6f507f15e4d2507c37acc2672f2a65ba89452cd462e4c10f0f53373265f61f83c987716330c5ad883c130aef10d535124188963915286248c63fe160a25aa04ce01182bdcf7cabffe445c9c402006fa1d9c9c12406bec7637610ffbdc0114419d3d1c2665984e38779b84aa0406a349297e54ec1a783c92edc841c4a5f8af3ab9fa54b24fb31dfd02339b55153b01c472a83b7bf729c6ea4d16268a519df2abfc77da516e51cbad5b523bacf2fa0510ca7809952648a79ee1749ae815455db8bbf5adc99f5ca08a2486c653e8ab649921b701814ef71ed1c312261efe82c0c7960e1aed0ac772a7a2d4a8ad5c72cfe4b4153af34aa62f09866423392fe1ee9158054e7877883c2be453f6f873fbcc5bfa785cf96646d7020bba6b16726f7bd76bf8e6b9ec886a69936346d9eef031cbddfef860b9aa276fc98d9e57b7282f0dfd2f4f6e22f9adecf6ec5acb74cef4d49beeedc4b607f0cc01b0c7750d3300d5ea95f13770efffea7ee9214aa608830831027a6cac7e43f5263b609ec5ac8392856353d8d543ca1f56c7fa91581533ba051a7521ea8b3406775e144c3f49fa69ee7c4b19d344a99df2abfad67aa357a685e092af3f27baa103215d1299e79bcdf523975e98d79bc8892bf67f091e78d11d8525ac973c7925330ef4a1f45f7e851fa464c16e2bc6fb8ea74ad9bbf6cad30116d6eef0e98654be15e71c33a9d6a54709f9cd192375a7b68ba8509905f524396ac59cb99b80757cbd2ae33093dbd51d426ee10ec98b966fde1e81919bb727d60f12444e546317fcd852c9fa41a622735d32f28716c9a7726dcedf3613a7782a67888c40f5bbf07e18f69a29975d88f645a878b8f9889ef2f9c2f2aa6d5e7111be9e71825db4ebef6375bf9e1949e7f9a264a731b9d57aa9d548c58ae610dcc797a805e9e0920b0d405ff849d3737009e8af53f45acfddc95f16a36c40c80bfe6ded1d71c9670466827f1f502fb36485df66b7c3d35669fdb34dd9ed97fd3d78a973eb0c1c4452f212660cc155545bc93f3755f150a56e0453410f37a721e465d48f09b5f26a97356cac9cb176f957f8f0ca7d01518275b5c9cf7a3eb7908dc9bc84ee704915bb4353aba2bc01d9b2277fc527487470d429f45f8dd2ac154d9a24af8c85be039e5a0125f95414f1b6ebdf3507abe4371059ecb17564fe60829d393a4af4dc91ba02869451ba5579a726f8f43f23315d143b465b436cbd5c65c2c7eec76e99ae3d1e6c885f7b9b56d079db9fff7d57d7e43d346056b4b3e80fd41a4ab83bfe3924fd91bca2b0a3fe1098961d9770959672e55d1203cce4573c60180d7b351eda4a62588777c77125f2f3045fa5304178bfee869bb89570f6119d16abb5e8f7334266864d5791cacd655e1ad9b2b9cd60aebb5d2b538322818315e3bd9fd793f4cea6925ca7c363d2d245170abfcdad50d221509fa89e7083c4f92436dbe527a7f48fdd6c24edb36991e8874e83cab0406a0463b966ff376f194e14c4171a5b05d3cfb4cd69e0512e063ed87e32faf9f900afd761f9e7858d96fc600e3e353e7bae4d0dbe455f6f5b9e31beef4625537273988514d2088e8d79c14162c29955b91ef33a8467208283ffdd0750fcbeebd6c621578582e408665419705c9a3495ac8b9ea9595986cf5cc03579bd43d898e96c55cc5828691b5f8ea1f36ff4b6498391e761a46861962c1f4200a5c355694092bca1404fa88c536b029cbce2c0d1cfb86465a4a08ed0ebe7badc715830787d113aec15b946b8b7600f9b7c0adb7d76effac9ffe26b6e007506b1aeb48991869fca7f6a7d9c67ad1b9884307b6b93f4800a1eceb15cb4e3ebc394e77da220de3b227739a05094f3e4848d3199b2255ba431ca0dfa8f5625fba3725f9d3c514c5513c763b7caffbfaa43a77411e876ac8b94fbc56788a11804c31089994cc79d273068924c7ef9f5de11a4ea6da0f321316f7cf7774f5843712448c7e58ad97c914311bb6beb061eb6946166e1c98bdef8e2c921e63a4ed085d0db4693fa1addb84a7db0f7649c488528df6a9f1be1c05e0a37d7010beade3d0b66c1d085966df161e8adafcc6355496632bdbcd825623f88f18b7f1b9c2cfa949bf793859c51a57a8c23cbc7f7af5aa5155f1dcf1c71de23c0bfcb40a09aa4deda6050c8569ab2f5c537eb9e087c42c3a670c286e959f5fcf1e57393465caf598def15e14c588dd70884248da9c6b6bd44d54cc73bde72a23aa259d7b8ff77d8ae97b3150e021245ddf4ada65661daf806e9d9dabec5558b7f550ebf7ec260b16b6eeca8b7a1aaaf9c5a26c0d951e22723402ab211f1e29dba840729edee9496582beaad4554e5e2eed3d11a14283c9e23ace5d2b4e433d0fcc3078b0124606cbb1603aec8f6f23415408e358da0a8b733edac893e8b77bef4f59328a6ae5d3ca87b0e58e7f115001f0a0c6214938f69fb4f9df5d94fd7349511c8be8f76872e109bd9bc6c2fdfff03993e49ed485a226b1da209b4d975acc32f9a900ffa6cfffddf31340280d2efa59844d59a7ec592dd5a87998b6113506c44c665ca197cebff1c90e5484cc8a6cb2c5b1badab35aefa35c1384f0bb6459061ad574c2f37f8bbbd2e8dff5f27f020000ffff8db4683801";

    #[test]
    fn test_decode_tx() {
        let data = hex::decode(TX_DATA).unwrap();

        let tx = BatcherTransaction::new(&data, 123456).unwrap();
        let frame = &tx.frames[0];

        assert_eq!(tx.version, 0);
        assert_eq!(frame.channel_id, 239159748140584302248388764660258118408);
        assert_eq!(frame.frame_data_len, 3028);
        assert_eq!(frame.is_last, true);
        assert_eq!(frame.frame_data, data[23..data.len() - 1]);
    }

    #[test]
    fn test_push_tx() {
        let data = hex::decode(TX_DATA).unwrap();
        let txs = vec![data];

        let (tx, rx) = mpsc::channel();
        let mut stage = BatcherTransactions::new(rx);

        let res = tx.send(BatcherTransactionMessage {
            txs,
            l1_origin: 123456,
        });
        assert!(res.is_ok());

        stage.process_incoming();

        let tx = &stage.txs[0];
        let frame = &tx.frames[0];

        assert_eq!(stage.txs.len(), 1);
        assert_eq!(tx.version, 0);
        assert_eq!(tx.frames.len(), 1);
        assert_eq!(frame.channel_id, 239159748140584302248388764660258118408);
    }
}
