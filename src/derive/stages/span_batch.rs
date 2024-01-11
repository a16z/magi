use ethers::{
    types::{transaction::eip2930::AccessList, Address, Bytes, U256},
    utils::rlp::{Rlp, RlpStream},
};
use eyre::Result;

use crate::{common::RawTransaction, config::Config};

use super::block_input::BlockInput;

#[derive(Debug, Clone)]
pub struct SpanBatch {
    pub rel_timestamp: u64,
    pub l1_origin_num: u64,
    pub parent_check: [u8; 20],
    pub l1_origin_check: [u8; 20],
    pub block_count: u64,
    pub origin_bits: Vec<bool>,
    pub block_tx_counts: Vec<u64>,
    pub transactions: Vec<RawTransaction>,
    pub l1_inclusion_block: u64,
}

impl SpanBatch {
    pub fn decode(data: &[u8], l1_inclusion_block: u64) -> Result<Self> {
        let (rel_timestamp, data) = unsigned_varint::decode::u64(data)?;
        let (l1_origin_num, data) = unsigned_varint::decode::u64(data)?;
        let (parent_check, data) = take_data(data, 20);
        let (l1_origin_check, data) = take_data(data, 20);
        let (block_count, data) = unsigned_varint::decode::u64(data)?;
        let (origin_bits, data) = decode_bitlist(data, block_count);
        let (block_tx_counts, data) = decode_block_tx_counts(data, block_count)?;

        let total_txs = block_tx_counts.iter().sum();
        let (transactions, _) = decode_transactions(420, data, total_txs)?;

        Ok(SpanBatch {
            rel_timestamp,
            l1_origin_num,
            parent_check: parent_check.try_into()?,
            l1_origin_check: l1_origin_check.try_into()?,
            block_count,
            block_tx_counts,
            origin_bits,
            transactions,
            l1_inclusion_block,
        })
    }

    pub fn block_inputs(&self, config: &Config) -> Vec<BlockInput<u64>> {
        let origin_changed_bit = self.origin_bits[0];
        let start_epoch_num = self.l1_origin_num
            - self
                .origin_bits
                .iter()
                .map(|b| if *b { 1 } else { 0 })
                .sum::<u64>()
            + if origin_changed_bit { 1 } else { 0 };

        let mut inputs = Vec::new();
        let mut epoch_num = start_epoch_num;
        let mut tx_index = 0usize;
        for i in 0..self.block_count as usize {
            if self.origin_bits[i] {
                epoch_num += 1;
            }

            let tx_end = self.block_tx_counts[i] as usize;
            let transactions = self.transactions[tx_index..tx_index + tx_end].to_vec();
            tx_index += self.block_tx_counts[i] as usize;

            let timestamp = self.rel_timestamp
                + config.chain.l2_genesis.timestamp
                + i as u64 * config.chain.blocktime;

            let block_input = BlockInput::<u64> {
                timestamp,
                epoch: epoch_num,
                transactions,
                l1_inclusion_block: self.l1_inclusion_block,
            };

            inputs.push(block_input);
        }

        inputs
    }
}

fn take_data(data: &[u8], length: usize) -> (&[u8], &[u8]) {
    (&data[0..length], &data[length..])
}

fn decode_bitlist(data: &[u8], len: u64) -> (Vec<bool>, &[u8]) {
    let mut bitlist = Vec::new();

    let len_up = (len + 7) / 8;

    let skipped_bits = (len_up * 8 - len) as usize;
    let (bytes, data) = take_data(data, len_up as usize);

    for byte in bytes {
        for i in 1..=8 {
            let bit = (byte >> (8 - i)) & 1 == 1;
            bitlist.push(bit);
        }
    }

    (bitlist[skipped_bits..].to_vec(), data)
}

fn decode_block_tx_counts(data: &[u8], block_count: u64) -> Result<(Vec<u64>, &[u8])> {
    let mut tx_counts = Vec::new();
    let mut data_ref = data;
    for _ in 0..block_count {
        let (count, d) = unsigned_varint::decode::u64(data_ref).unwrap();
        data_ref = d;
        tx_counts.push(count);
    }

    Ok((tx_counts, data_ref))
}

fn decode_transactions(
    chain_id: u64,
    data: &[u8],
    tx_count: u64,
) -> Result<(Vec<RawTransaction>, &[u8])> {
    let (contract_creation_bits, data) = decode_bitlist(data, tx_count);
    let (y_parity_bits, data) = decode_bitlist(data, tx_count);
    let (signatures, data) = decode_signatures(data, tx_count);

    let tos_count = contract_creation_bits.iter().filter(|b| !**b).count() as u64;
    let (tos, data) = decode_tos(data, tos_count);

    let (tx_datas, data) = decode_tx_data(data, tx_count);
    let (tx_nonces, data) = decode_uvarint_list(data, tx_count);
    let (tx_gas_limits, data) = decode_uvarint_list(data, tx_count);

    let legacy_tx_count = tx_datas
        .iter()
        .filter(|tx| matches!(tx, TxData::Legacy { .. }))
        .count() as u64;

    let (protected_bits, data) = decode_bitlist(data, legacy_tx_count);

    let mut txs = Vec::new();

    let mut legacy_i = 0;
    for i in 0..tx_count as usize {
        match &tx_datas[i] {
            TxData::Legacy {
                value,
                gas_price,
                data,
            } => {
                let mut encoder = RlpStream::new_list(9);
                encoder.append(&tx_nonces[i]);
                encoder.append(gas_price);
                encoder.append(&tx_gas_limits[i]);
                encoder.append(&tos[i]);
                encoder.append(value);
                encoder.append(&data.to_vec());

                let parity = if y_parity_bits[i] { 1 } else { 0 };
                let v = if protected_bits[legacy_i] {
                    chain_id * 2 + 35 + parity
                } else {
                    27 + parity
                };

                encoder.append(&v);
                encoder.append(&signatures[i].0);
                encoder.append(&signatures[i].1);

                let raw_tx = RawTransaction(encoder.out().to_vec());
                txs.push(raw_tx);

                legacy_i += 1;
            }
            TxData::Type1 {
                value,
                gas_price,
                data,
                access_list,
            } => {
                let mut encoder = RlpStream::new_list(11);
                encoder.append(&chain_id);
                encoder.append(&tx_nonces[i]);
                encoder.append(gas_price);
                encoder.append(&tx_gas_limits[i]);
                encoder.append(&tos[i]);
                encoder.append(value);
                encoder.append(&data.to_vec());
                encoder.append(access_list);

                let parity = if y_parity_bits[i] { 1u64 } else { 0u64 };
                encoder.append(&parity);
                encoder.append(&signatures[i].0);
                encoder.append(&signatures[i].1);

                let mut raw = encoder.out().to_vec();
                raw.insert(0, 1);
                let raw_tx = RawTransaction(raw);
                txs.push(raw_tx);
            }
            TxData::Type2 {
                value,
                max_fee,
                max_priority_fee,
                data,
                access_list,
            } => {
                let mut encoder = RlpStream::new_list(12);
                encoder.append(&chain_id);
                encoder.append(&tx_nonces[i]);
                encoder.append(max_priority_fee);
                encoder.append(max_fee);
                encoder.append(&tx_gas_limits[i]);
                encoder.append(&tos[i]);
                encoder.append(value);
                encoder.append(&data.to_vec());
                encoder.append(access_list);

                let parity = if y_parity_bits[i] { 1u64 } else { 0u64 };
                encoder.append(&parity);
                encoder.append(&signatures[i].0);
                encoder.append(&signatures[i].1);

                let mut raw = encoder.out().to_vec();
                raw.insert(0, 2);
                let raw_tx = RawTransaction(raw);
                txs.push(raw_tx);
            }
        }
    }

    Ok((txs, data))
}

fn decode_uvarint_list(data: &[u8], count: u64) -> (Vec<u64>, &[u8]) {
    let mut list = Vec::new();
    let mut data_ref = data;

    for _ in 0..count {
        let (nonce, d) = unsigned_varint::decode::u64(data_ref).unwrap();
        data_ref = d;
        list.push(nonce);
    }

    (list, data_ref)
}

fn decode_tx_data(data: &[u8], tx_count: u64) -> (Vec<TxData>, &[u8]) {
    let mut data_ref = data;
    let mut tx_datas = Vec::new();

    for _ in 0..tx_count {
        let (next, data) = match data_ref[0] {
            1 => {
                let rlp = Rlp::new(&data_ref[1..]);
                let value = rlp.val_at::<U256>(0).unwrap();
                let gas_price = rlp.val_at::<U256>(1).unwrap();
                let data = rlp.val_at::<Vec<u8>>(2).unwrap();
                let access_list = rlp.val_at::<AccessList>(3).unwrap();

                let next = rlp.payload_info().unwrap().total() + 1;
                let data = TxData::Type1 {
                    value,
                    gas_price,
                    data: data.into(),
                    access_list,
                };

                (next, data)
            }
            2 => {
                let rlp = Rlp::new(&data_ref[1..]);
                let value = rlp.val_at::<U256>(0).unwrap();
                let max_priority_fee = rlp.val_at::<U256>(1).unwrap();
                let max_fee = rlp.val_at::<U256>(2).unwrap();
                let data = rlp.val_at::<Vec<u8>>(3).unwrap();
                let access_list = rlp.val_at::<AccessList>(4).unwrap();

                let next = rlp.payload_info().unwrap().total() + 1;
                let data = TxData::Type2 {
                    value,
                    max_fee,
                    max_priority_fee,
                    data: data.into(),
                    access_list,
                };

                (next, data)
            }
            _ => {
                let rlp = Rlp::new(&data_ref[0..]);
                let value = rlp.val_at::<U256>(0).unwrap();
                let gas_price = rlp.val_at::<U256>(1).unwrap();
                let data = rlp.val_at::<Vec<u8>>(2).unwrap();

                let next = rlp.payload_info().unwrap().total();
                let data = TxData::Legacy {
                    value,
                    gas_price,
                    data: data.into(),
                };

                (next, data)
            }
        };

        tx_datas.push(data);
        data_ref = &data_ref[next..];
    }

    (tx_datas, data_ref)
}

#[derive(Debug)]
enum TxData {
    Legacy {
        value: U256,
        gas_price: U256,
        data: Bytes,
    },
    Type1 {
        value: U256,
        gas_price: U256,
        data: Bytes,
        access_list: AccessList,
    },
    Type2 {
        value: U256,
        max_fee: U256,
        max_priority_fee: U256,
        data: Bytes,
        access_list: AccessList,
    },
}

fn decode_tos(data: &[u8], count: u64) -> (Vec<Address>, &[u8]) {
    let mut data_ref = data;
    let mut tos = Vec::new();
    for _ in 0..count {
        let (addr, d) = decode_address(data_ref);
        tos.push(addr);
        data_ref = d;
    }

    (tos, data_ref)
}

fn decode_address(data: &[u8]) -> (Address, &[u8]) {
    let (address_bytes, data) = take_data(data, 20);
    let address = Address::from_slice(address_bytes);
    (address, data)
}

fn decode_signatures(data: &[u8], tx_count: u64) -> (Vec<(U256, U256)>, &[u8]) {
    let mut sigs = Vec::new();
    let mut data_ref = data;
    for _ in 0..tx_count {
        let (r, d) = decode_u256(data_ref);
        data_ref = d;

        let (s, d) = decode_u256(data_ref);
        data_ref = d;

        sigs.push((r, s));
    }

    (sigs, data_ref)
}

fn decode_u256(data: &[u8]) -> (U256, &[u8]) {
    let (bytes, data) = take_data(data, 32);
    let value = U256::from_big_endian(bytes);
    (value, data)
}

#[cfg(test)]
mod test {
    use ethers::utils::rlp::Rlp;
    use libflate::zlib::Decoder;
    use std::io::Read;

    use crate::derive::stages::{
        batcher_transactions::BatcherTransaction,
        channels::{Channel, PendingChannel},
        span_batch::SpanBatch,
    };

    #[test]
    fn test_decode() {
        let batcher_tx_data = "00684b266f1d001e705ac8907c7eea4dc200000000576978daecbd05545cd9d6355a853b0412dc5d823b2438840477770b41437077d7e0ee1adc2504f7e02ec1094e709737d2dfffc6bbf77b7d3b97249de6efe6f4e830aaf6a973aace5e7bceb9d65e7bed5a604c17302e2a0aa1646f0fccd5b88e8eec8393e3449e858fc84edd68c88e4f1bdbba72a351750cd217464ce2da180a07d9f77159722050007700be3b1400040a0408008282808281838080028000000008047cfd0f041c08fc3f2f4081401050101020381800040c08040505fecffb20402008000804fef61700000500417e7b030cf0bf8e4778076166126d1fdebdd970255f3ae5a2312a20c671a14d8e00ebb2d926975f4ec44b2cff984f05584547c0d71b9c2220215b6057f466adeea1991897f9d810f5d4dbc83451dc0e85b8a53c4f5d3eb5e143d7c9436f921195c44e0bbf17716426883e4c581dc6118d230f8b338ee43a31031a041668998b13bfa4cdc31410944cc770315e870592e6be77de4b861d1cdceb135f125e9507d3627fd58e278c461820f0a0aa75894e4993ae5e6c03eaf889d824f3caf3b72c95b2521695340bb5da28dd502092349d5a3a697a92d201400bce5aa32127b951c9a4acf9b760dd3cf6a18f725d85e0890d77324e74202e84155f0a497f1aa5731a5820ea0c93bd808b83d42abf4eefe12a632a333e90577bbf163d5690989d29c80ae9d9c108b232f932cfa387ca5d6014fc42c32d1b900fe0a29a9283183e8a7856fcdedc0c4ab3f8e8fa3157a173f3c8c08568496e9a133e4e84c1cc164fff36e2b21d52c98192b5f8fc2632917003a7aa4e1f6de5aec6d22188ac54718ac65977fe2101c51ead5baec422ef26436cc4d4175ead047565de17d87b9117d2f546d62accf2d21cbbd65a7a656432274f500f04d999601610d77cf0f684f8f514af908a2bb967b83afaeb9bf35b960baeb4eddd991625830c2673572e3264a094a9d25581a27b62e9c7d6c2c3575fc8e397ab5fbea8cfc5bdc238c6818f5646446138fda07001c9c811ba52970c22ef11cbd14cf5d95e7cd978ecd468d009568b62a26fd42376349f2345516aafe83490e34594d6d5d46151d8d638fb2a49fb52396abea7ce8d76aa7ba06c871c4b64fff9d1c0605833065502aad9c3524f5441ebe628eab84fdcf0593df5058316f8acc8e68ac749c5a765b2239b70134d5f88aea537f55c827169b61c746fd8c6553b759ec31023396984854c2b673527e5c15571aa55171c0d9535206e67f9570c32b3415126f52ee8d054916729d90d3fa6ed714c3dd2e30913f5ac7d175768e50471d1239c808354e8b8d707ec788351b4f4513b31705a545e0b8131ed706facdfe7c83ddf685efc98b3fccb0b78637137a6c7ac29eeb979c11c2827fcc7d18d31c06a69bf53102c5e6a28d81d757a3eddfd0d1609ca06961617310f802525f89b0a869b0afb07bb65c3b90ab528d2158b8cc9ad9b8bc6d39fa51de5569b07ce48318ddade3fc38f9d972da13ada83e8b13c8d29b51b0911c2056ff7080dced703eb84cb67bf44e29105cfe230d2efa50e350262107c862a9a4759ade709c42a08c814176d08ff02437399b19d876a43ca41d10a06ace0716ed9a77ade7e538d788ed5adfd4c748172080e0746fb31d10c32eeea431f61335fcced2d7fd2a2563b4334a4a98e882c50b7c65e1aad0f1e6225316862945a051a9fa033a546664d06074bc5cdf84e213f322b98b9389b6c743d4aaf462f27722f8a01e8ce23436fbffea467ba9f6df0f9301477c64e0af791fe729159ef704a0aba19262ce9feec7b6eb8aa77b1a1371ecd2c9ed52ad85ce20386a8d0f5e7953285abbb914a884f2c95c158323ce6ae5f4d9da47a1838a70f8106e91b1486e72be4e7611e5e7d167d147481c5d4553a6372625b7bc141ab991facc8efe8ba7db0b19742df92c60b1d4a443b3392a3c19f4fc0df7633571fb98c3ae4cb61adb94a384bb03c3e0b46a76199fbecbab2b0ee0273cf0d82be05d085e1baf2b9c939be28d060569e6bfd627b327b5c25a2eff05845f0013c7c3470998e263af13d404b086a3a728d3471b2d32803d603231944a350426fb5ce5402afe8c81f13243ddce74bcc1eb752663b78e398a008d0f35554c9accb249186dd38048e257e6b8c839aed09edd2971104bf68366b3114ddcd2e431ce6f2997dfdd7e93a4708d5e3ef4f4d3a86ae76d64ab4cb1da89668d5512b9e01d524ae9f90c3b95a57a2ae5dbe063e452caf97619d7071d206d77fb61267e5022eac9001f57cd5fdd86c216b49875fe7d34ea405459c8e012a074e6208e024e76cab7a98a7d5deb88af5adab3ef0d4b7b5b7623cfcba3c11fc118fe442d53bf7e9c896010146dac5553dfa3c782e161ec3d33abfea8036e4b474f46d75fca5cfac4abaad9f920252233c8a7436fd02a2437b400cb70a8ec3228efcd60c30d1a8b8c6ad0f8750176e8850178f133095c7e0300c7cb663d464c06641bb409d3070652410b0df735720a7b567a17d72d09406d73916ec80bccae6266b32155fe638ecf57bd602ad310f8d157a988214edc6c6a7e9fc25a18612442710948fd662dc5cb3f920d8d53644a04397062b8e2e808718a2effd3b39aa2711f816ec00d36a28b21a2db368848f73c3eb73be9295ccaf647948785a592a702a19cd17edd6ec5acddee191ba31a96bb8161389c0a67e8cc11536ef20d38206e1a7a83a9b6d2e3b739c321b4ac393b8115c654123281f2406b30f5c00141af9085205bcf1b0d27757cbd77c009e182f3d6b5deb2cdcd3d70c09d93465d6c7eb7b242a9055df576096d1dfc25bef2480c2e99641bfec0c171951e89996085fcd5ebb8252284f1eafb1f9b41844e2758f3fe2eb03910bf0d18da281d7e283aa07c308ea32cd3629898a1de224256f80983eb28989303c81f8451aafc3a298c6829363c4939f1adbc28c5ff6cbe436c0c48c9f7cb8594089e0c9d062b5362967221a919cb2c1848118a1c715accb8713d65936a75bb42cadfbc0259022864168768d42f6ae4e2376b9e30343d9b6683ff775012ec1ba9c40866c359feefc630d68643c568f050fb42fc7605cebf22be502a3ee6f0c935517d0e8de79704bb0ca3c25a515c96b7bdf6fbfbfe71068e09a821553a736054aa66c82b01517ceb2f026c942df1a4de4d2813f288c843f6f68959f79db23c13799ce5e3a340d36d32a9b94e9863923aaf0addfaa4e602183ee5833756d6cf3285bd8e7b9535a57728e13eb67812088ed026ac54ef69b9478a5122a3df90d2e9943cd2dd59ed3c8596915fbb37dd1502da0b213d1ebee8f507167f9637013dded695107a9e1f0bcb0b55957a9298ce597d1ec048e79718830c32cf900e61030a78fa0c5751bc7ce443ca5a0924afc84220bb03fadf43457f9790bbcfe7431d031b2f83ad0b5fb02f3e31c3fe26c7fec738826fcaa59a0c51be1c176476c70d14fa7f68c71f95a105804d4f1d30342b390ece023800d457584f978aba7cfeaae77ba4d8f94fb9018b26586fdf661313e4aed51f10d6d2c854900418ec874cda1d4f966061a18d09c695e459f9e646dadd332ac97a5947763eec673880ac8f6157a166d740053573ebc9f52935ac0b56e66b448a1610c15e2d9e5f2a1172d69023784bbc8a71e75b66678c76f713838157761b416ce195a05f580aacd287c9515aeef06a11d6ae849a3b733651d04a80bf3e28c85f69e492ddd0e504913d478325357c706dfdc401fa395d4f301ef815eb825688e07c695ecc154b25e8f96f287d34eb2eb9d9a8eb0a88d620215ac3449abc31cd7352dbd27c97ed57ac426f5a73626d50d3b481fb8e1d0743e13a45d93c45c32e067ef82709df657794f90780d904c3fd421b33a3b7b788464ba39f531916dc0d50f83e241329fab4fd80428c0f0e4c581cff607088b8aa630cd17695f1a666ed48d962b2a4a4bc3d9cbd3e1b0024ed002f69bbdc75379ac8f49a6d61fbe42d9af345837b2c07ef6227622c73c4d1649b3b92ea1fdd34e062bd3934a44f009443ea43561a519ae58b987a21aa50c71eb4c955411f4fc0c3b4b73a3178e6d7b66cdd583f5a86c0f07f90a5b36edf53c73afba976be9515ec829933dacaf7948a677fac075ade100c3021c53d310c689a73b7cf87c795847bd56b1c925bfd76d815d865e0e055297bade438a9ec62280bc6119c52ea383ab701d811036968b01e40124bae51131545097f88d7a05544dd6f9a59a8eb2be0cd947ab2d296a89f3f7a27861743d039a90d695ae9a15982c22fa4c9f4feca8748bca50e9e1b9d730bbd33e09148d22060e847fd9697aefc75c194cd6ebceabf9cc2193929c737bf9f979c0bce79c4c4b90f69c38ca3622a5b6d82a3ab065666e17d3716df6cb871dabbe6d4ef027a8fa4f2096d6d7473f197046d04ee73b63b38c1d4d1a1caacd3fdd6d9a0ee1654143bf0aacddbf686692627d97881d02f478811b06aeb799029bbdcdcb92a07db51df39a117473c3b72dce62ede0fd2aff61ded042da0091a64c09e75bc2d5818023e86a05b901f4054bd1b0de9621c12fafa939e99493ac5eccebef404ee7d6ec7fa6441bea1e3cbece3cc027ad524a293933dc0c6fb30ae19aa5ce3f7fac1200016ddfd313381586c08a2a221dc397b292021fd002a2b02edcf249c58282cbc3a9e65d622f854773e50d810a0c9603063a26bea55f6edb65d7b61411e3eaf26e9062f902afe91b210032714ea953843496375383b696551bc622ee5c8ad580446debba5a045d36d80bb82fc88792963011b6831a16642604bdcf33f590ceb1d87260d84643a277874d8238421e9833b52e9c1f3cea84b771dc76c5b0f648dd00c3c4b35a86d6e160f7b7bcae7d3abe5f1467cbd0a884c35eea5f50d2a266da522bb71ec93fbe413339188a9d1a477e52dae31683a68253e8e8a8da74cc0bc71a4ed808da4cb1ccb021ab0ae00b7de901cc27d8d8a6639bb725b0571a00976aed601fc22f6e3b9e76cb7e267ffdfec908e778b86511ffc007e150f3c6806b4c3f826b3d8807c9ab85eda9d5d419a8c444c2168d1072282b4f2eea13e589df8abe4b6153d9cfd831b173d9ec85e12c62bc728cf5e99e182ad34717fb68bab2b73731491d7bec6abc651b0eb79e1ed2f2f90debcb34a09aad81552949145c42b333cb06b37ec6b93d9f588195aa79bd2a8e771a282f312d2ac53429d1c246040e2a7906a9c15bd0cfb686e1477904296122150ff7d474a70db20227fba2415ef1c09996179577a1ac732421a58d6bf544eb553634138c67e1f6db736f75264012749f6d7e62dcf43b75d0ba10c61738b36a8d9254ed61933e8c9a41a56a9669431841f1d3c07d7edeb578dd9ace04c67e891ff794046af9444f24c9b3ea351342cebbb14e296894658ea82341dbdc9dbd4dc1809b703a0ed481527447acab711591de3ca3f84c9ea1f41488962d637443ffb77a224f8bd79d97a66ee4aafa853137a97db09b4153da271e6af7247f3cec962a4edad651173b9dd7cf626b59912e60b4b187672a58d27e38d82e9731aef8b29c4cedf51b70450bdab024632c7dbb787d57f5fd0f4a6f942b1903078d7ca2ec50d73c80cb1fcbb41c48f4929308b50c3de990ce48e0b3c7eb42c240870c643e38f5b9ac4e53576fe8af7f80a37f26894a099e3d9a87bf67e3eaf0200deb459ca38ca118852ce854e3b4d7dc84741a41a663c2635c6bb7560483132addbd14745bc5083df2c7657c40102bca3680a9c19156526d83533d340f2ea7d7eb80f0d8c3403cff135114471cdc93558995c96871feb5b851475985caa2fd07d4ac03a0c34ae7940408de5822e40c67db5ffc741a909a72ac701ca9e67493585fb75b1206c0e317718314eca91b3c202a4b77bbb6073da9127c92fad1346667ad86d685f065bd9e1e0926b780d49b576903c969effa17f3d1579740ddc21d1e4d196ba39e847b73f60ae1108b7c3154d1d047a77a1ec90b0d09412df6f23c8822c2fab4834479f88d75a1e21ebc91f9f8447fd78201954d77e7e33a240bef1d8aa5c135cef1bd640f1f0dd3686798161e4c6346e235781a09abd56e0b15bee7dc3be0e8a8e6c529ad8ea22a0e4c87c259ec017b0952411c2191d478cfba3a950b85561f212967d184ec5641a1945ec6bf5b17c16877d19cef89da5836a993d55214e23d43132ccfab5d6f0ce07309ef61b1c4b7dae3107f7e43f7403d4aeddcb21a61677336a809e1d4027acb6931e406179757caab5ea3173a33c3b17872a0e1c4f7d1fb2948c6cc475aaf75fd185a1696a0a76cd379e412cb2590b8bd98dbf2332210a7f73e3ac53e2d0a14a18fb3d7aec445077a137cde853a20ad29aff4e6d8712883ad53539da42a61b911b04bfd70e0f962d409a0bfd03c4ae6232fe4e88de3abbb6b80162ed6311667e30a5116033c18713167afed873da10f17eea266a054489c78a1895fb532bd7ca4a8194782707563b0125283f6e8e3f40c15c278c613e296fa8f5ae030ee3565306f6babd7bc505a20e5e54dddb8fced79124de746c56c14279574cd6a51c10d4720aa30f0318e02b542a84f92653ca363343b444c6b267d3aa7f517e91c6f86224b4900f5215c00f3e9e45167dc3d9085bc1322dbbe21f210149a6ac3e238d6c10bba99155d3c03ec7058c3b39d2fd6d289964296e22a37d23d8f9107856d16809e0f259f51b4e942283da4af965c4fdddd7c86284eb1418e2502190ef5d639fc635cff831c97d285a6422e7aac2b5c25aedea4dc2b3e647db44a6dcbe1aaf2dd60c59596c742e87bb65e925bdddc91c89d3a28445d0a659a7d58767b5932197e190f9bacf1bcc117a6866dd0e960276341d8e5b93e8da8e2c6926b47a535ebada284a9e254b94112330ff081d3d2e818fbf7b22122c534dc28a292f7ee838a973b0310f1f5c2ad0d9e3ab63c8af4ec26737535fa021a6b3cf0be1aec51704f8da34d0fcee61325fda0aea526270662bc935749f9bad25779958a61c6687c66540ae92ddb8c64367835fcc79f066b3e75f76514a65b59328ed2c670047ec1ddef7a7d8eeb2a527271220b425f5ace21f611596e22990ef19d70d999aaf64d2733ceee7ebbfac262a4593799591291a3274b8a18d73efc8e4d1313576e2f43e6fb5141e61045b76c5e8a9c06af8d0282a75e450002e3fcc005e051c83b619c68469521f9c5fca84436aea05994b7c61156ec31562ab4870f7967452072810cc8e2b0ef6db7a429317589e805f716e417c7a08bdc98756e4a571fe1710b3850c44fc0813eeecaeb2dc5f282079d2676304b35dca31d32f1aa0dd76a7c1c4a5364eabde064353e40ed00da95a1a6c03c08d3359338d7574d366c8803aef1444a26ff40dd524c2be054b95e3cd8771ed3b6eeb1707d6fd2e449e50913223929d4fc553cc06f72b7bee91343e3f2937a78279cfae318b767dc57feeae9102c6d8e52142f03c262e2ede000c243bed229de36e147cfb1ed9bc731bff830b089ea310e5517354c7ecc672fd775dc0564ad174ef3f8ef61bbd88ad11248d7751cdbd32cb9cacd600b6fd7daefa69fb909a82d619e490b4355d9c14cbdc26c4cd49dad81d982b40adf03d090a95310633ca18c340c5f170c9dd6c3547e9e7658bcb4247be63138324f19ef616b879e0e87f8189fd25cab864ab249c7115c6ba5af497c1c473c175a19779bd49bba7f12211f2e04825c43887a8a9f4ffec17aaebf8f3cd76320f8b0059787ee85b311257bf4ac570621a65b128777df4677bd9b6c95fde645704005ae791e12bdca825d9c33884ebe2c155496bf689279dd91b6bcd803889694bad25031c6e0e0163128af1946d9b816d4e1a8c5f92a0e07c5699df571c4c3910ff0d8cd3b49af29207d853d1df8449f0d70191e7e2a785cbeec5dc77c05abc2702dffc43e4129c58750b5e0f93be6bdf0c7c9cca2ee4f8ee6620b0b27352809c449c453071df2c87ad6e959a3c9b85f35ec5d301dcd478cc479441240f5e1620ffa1f295ac456f0a6ec754af54455cab5ad335aceba8eee070b6c6cdacae7beb5b88c399465b694e84e5ff5c67fee848b1879ac5061b887c0f208cf4918dca24cfffdb3747d49d67d3fd295a50e424d72bef725cce091109d26a53911051c0b14105741efc068cbf0bb6ab932b161b2df8e2d0d073284cece9e270c4648533c94ab8d17a321f573dcdbb2ab6e7cdc3db125eaf4a1621fa910cb13c9c9f6b837710a6e2d918bfd746c3bb8dcc298d09be0ac9a5fc66da81ea67a17bb14b21fcae85d832127ec90be36424a1ba97fbb6cecb963f49b8aacc1b75ef90b059a606490da91c8be3c37e6aacda3d1f4b421027dc23a11875cbbe192bd5656a25ac5a374c3e54e17ac15854c6410712ea25f9e2fe5d6206a1312d02d83a05a247e819756123bc58d3c25f063eb4ee990504b945b84e3e68a621d1081a284db43ec9ed6e141deca68829dff5c04f6b64c10b5d262d98c934b98ec99e22854de887719553328d9b1b38541d436c4710efd53402a3968a197f6e6f625270f3f3795c788ddba994afc38a6c5838f8a5adec1500f83fccb973162e6aaad25b95d31022a0d45419e6b6ff84f47697e4e0a611dbde054012e47894590179767b129f37f24e111cf5795bd097049df2138af910e5b87661035120597828ff517f59727dd79bcb1c61db97229e1465456f028e768ef6d2091d0626a95a8f14094dcd4f3676e88028987f6e06195470f9a2b1de777a83924087bdd365a09cba325c767bb5e63053a453eed04f1e60cdec2d8fe784e9614be0a8f261961fd3c209b55d5a36bd218ff8b59b6aab10929ab48574cbaa41b04a243aaee28f5d62a95314e3b424f5fa6c1430745e21649263fe8f395cf1697a2bc12f40fde75ec7919ab396c37ae1d8ef9d701bb558e644aa99e2bab1b3bd3e7f7909d710dbcf3a345dff37aabf7d961c177f93dbd68c1839cb84a4ccca48b1acea8bd8bd57d63d5999a0828cf336e88b47e7a0a4198c9f10fc45bb69b7c0704b484d672b1ee15422a902d8b68b18f435fc035bba949405a70a36bb715d5b4b0193b63c6afe1e2b6cce5243c293077a0c741063d7dad8cc25efdd4ddab9e3db8fc31b321d79b5019c514904ac4ce270f5e9a242456bf7cf84e74493202d290ccd5f74ba0ba7c8822752c90199fa97412a30bb559911a0ee349382d2f8a3f5f948b4af00b219545afa0aa1cee3d8b33acae9709ec17885d84cf5b7baedcd85ec57904e00b9f5dda385b3a309591e16ead1b9c84b587bc24b74b69ce6469997a336ef85c137393e9f4ad47e83b6fc23c8e2c3188d8f7632966f474529cb641d08684c66706e0626a850e2be72f911fcccd60f2201bc3aa8b91398031fb2bd497bc11a054de5e16c8350dc833f23297d1e12bb3652378b0b359e067c547e5df4814caf4122fc384a09aa08c6eaa3fa3e0c397351dfbe0ad048166f1cfc3a6ea578adafca991de5b17ea959c94002cd6f1e230108d46e7ad978f3fe23ae06e9d92d048cb1146e33d3dad2eef3c657b152a14ac96299caaf72c92575e2e8da06bc136bb08fd3cf0797e0e600b483e20def4ec5a15463aa52b352b6685e552d28a3640bdbdfeb97162461a146ec85661049422d3a08d379985570ac59e9d0726f78be19750151516163eb9e8b8734917f028518ce8ebf9522c1fb424521936c360428c13d828878d1cd0a103b9ac11d6525db55ee62e47307d9c7e9bb082d3e4d22891a6e0c52ed40c437653c968924eeec9eebf45e472e9da802e4be9a13b911c2060cab7466ac3edf570e2a3364f195c9d1743ffd036b6b15a73bb741b6d8142ffd69aca8c32bda39b901f8fd0ba29a288f427fd1b506b47f088e2268d0ebff58f9ad0f57033b908eacf422bd306cf216c968110722c9fc0397076d51fc88385bb3aa0051b6cc2f10674428bd785b40e084d6aaf9c75d1b13655cb3a3d542280dbe6a2b1f30e80bab9e2dcc2d88196ef7f26020d6e4289086ab19fe26c29117f10b57afed02fb7e8f3993d9da709a3e36ed2972bf5c923b08c61274b089ba5473032457a41242f5fbe25a23c8e701d33ded869cc5f24f8685a74590dd275a0ee10a1115021dd140a144a8266e535c1dd2ab29de0790af362b98d06d0dd1b2d7936cb6f7ca18d57f8a0aa0b4e0cf74414b8f67ad937fe897c74860f819e82d3f3e2d30a8d06e97d4e178e731110a9ace893bc14ff15b20b40fee8cd9a7482383a5f1e6f47908a033b3296a924b6edf43a58ed412f14e7892ebf74b7489bc4e1639f2ea9aa2a8937eb0a8eda11b65e74276d8be960366fe118153d184c2c678b488cb38166feefe55ea37a683e3ed2c7d88d19dd5a3163228e839a288f509d22b768273af7923f7e2734c1a5e52137446f808616cb7a6386bd1890591710291cba210b25585f960284479bdfdf0c874a77b1856752186b984c3f73866f025f387f931e4f3f9b70660dabfb31f86910ec72a8615aa022594c9a9dc95ce5eb8fd4047598263bf2e29641c649e679c331abcc346c594615f4b151a2975a1d63532b4372824aca2463274984a8fa855e5c3de4f07d52393957ed606ec3ee7bc90eec36cc52e003385833929bf30f2d0a0e2a711ab8a2d97bb221563f1cb7b51bbfed327bb91c024ed400dbd6f2da97341aa928a04dac10f17d8faf4847479c219472a7eb531bbbc846dab03a62a9e9b6524bc1c718a71b9d708872414a5c42b073f86dd86a75e98c4b1dd6417378e6569955136537255800c950688228644a691c0a25968018d17a5bde681e5dcc31d1528512c98d4c13dd95b360093190b5d24d6a0a262636208c7e81ec1e1d91aaafc478ea9e6a2b369c99de1349019519e3b70fa7356e6ee663ce9548ac098f3390c7f8129d727c4afbb79eb22a4e90f7aba25429abe25966ae5cbee390db2b99c314647f6c407c94d0f1d9c4156d2ce7394ed93874383057219324b32a9b1caa3611a9c5cf405c3026ec47deabd825b008ce22e462179ea6cec2d122828c504ffbd9f7b84bef552d132b4e834a1d239b5851cb3ed8aa4541743dfc15f7dde4351ff244804561f48d892ff28ed38ca3ab0ef9bdf320b64a4a9cfdb8604e1300732e590e7d4acc563bffed3d0a02d7d23e7b258507945097a7bad33326181f98fd6cdbd8f731797e7142a483f9e484f5cdd4130b0b1cae841fb99ec61eeb48f155eff3997438c72367057f101332bf1ff9cee5cfe24ba12f9b51dcf2eae9e03890008dc86ddeff34bbff3d3b908920f22bc4e47dd6a0ce5281b50c6993c394b770122cafa5cadea7257b65d1c71a0bdebcc59938f7c05a3a13e5a8d5bf8efe9533d36a882ad10b72de9ffdec7fe43d5d7d648d961c82e460f7eb251b4e2adf3de7aeca9f7d0f240a2acc2c2f838845e704f11805006a884312c68f3c97bb84cfbff73b5265944dd15d4967984f3f57b9d2eb348e75c0a49ef8f4e924ce5e4148b4ae94486a846f515b1c1339818f5acac64e7c7c792e5e313da276c495c08df723dff9bf3def83cc8a4fb43c68ea5368a2813c13d636a2a7e39277696cfd8afefd113bf86f9fdf7ffbdb7ee4bbfcded8fa6f3f2b12ae0332555c6a81ac68626e502710883039e8ff7be7819c818c7b0dd3a917e97a80c2f378960e68edd680b46621947702fef870fd467bcabfbe98d6a7f20a0f1cd609bb4ccfc3b53841b6f1a57bf6c71f074901fcd001ec02fef607b0baca7ec92ad54661126678346949d65b399a38b4fcd4113671bacb1cacbc11062d2c971ca08a3a143595861fc84d7a1c3bc3fd78c916a657c0c39c3ab7fd1001a067e784fadb2571fff5fa1bae14bd2edd5f2e078f58bcf9e18b2f83873d17fea7c56ea813084007fcb9c76f3d06000034ffd4bb3c307ffeefbfe4759e9bb6e7c2db010464b0ccf74fb025de3e6fe8fed7f6cb8539ae7dda951e675a25dfaad19063c67494ec7f6dd7cc318ace56326d18d2a83204678bd8868ff3d0ffef9eeaed8f469033d0536f600891e698e6ff67dba0cb3fdbb6711fedb6b133a3d8445d9014f33031b18f0d1cb2fcf1c7411b7fac5b40d6ff8f6da7322b8cb9b0da35364ce4fbca4fb1aef0ab1ca8390a1ea4972013c0b2669c213a621a5a1dc3939d82b24d52aa1e974912d5d13a8c5cfb36563dc7ec2cfd604f0ad4b37342ba956d0ffc6fdbfe7d8b00e27feb27fccc761a7bdc068b91cfdb195d2486906241844edead16ffdafece6f0ff9c5353f3ffa9e426ad20979557cc14df47fba30451a31e6f1e3ff785f6830a13cecc38f1395dff87ee83fd8fedf1c3f36f6519734bf39f6ffcd2ab6fa24603092cc19b168a1ab581fd59bf5d5d713ffccb1ffadebdf72ec838c7b653ae1395afd99b4e6ec59368ad3eb2fd43da38c13b515b79dfde8b0f917d11a75c9e2d52a89492e074fc6a0ae2ed139ba867c1f0774f3950cd465bf671824266df0eb7cfa4c133c75801e9fef900d414d872479553da5a21773357349314f05e0d6437ff017d0da6f3d764f6bdf306d30b6ff1fad8111ff6cdb7e232613e77cbda0f030cd4a358fcf7581e6607cf98f3f0ef683dd024af17f6cdb680fb75e5915f6cc9ebf57634e8d2e03bb76478d90e9143616a5536fed85f504c397631ac8c9faacc10fcc5a5f1eea9db1e2e30ffa77a23da5a17b8862b59bbff5d5b6156f65dbb3ff25ad7dcb2ec17f26add57ca9018b372f8316d3162e6b1a25b1e45b4362f95e5ac36a323110efbc25d6fc8ebbf08d8fe0fe02dabba7c5efa7c533e0b6ab27f8d1e2690d70d9917d90ee1be7f37cc340be753faf7f7df112543f614c826b5b8d92b13bcf2e08635c28053321b6cd3703b71be70673abeca63ee60627a8285003695bce65af87b26211350800285a3cb792f8dce09fac16a98dabe34e40cbe277b6352d5ca79d04c6d98722c5e874a0c6cf1cd4576c563cfdbe0422d6d9fa65fc50a94a0dd4aab6e6f625c4280c3c1bf6f71b4311e8d72f0000bc24fcd777a5f2c32e0829209ebf29994a885614db7587d28d02008019ff7ace55587bfbe061caf1380fed47294d8e15de5eac6b907f7b5e547010c9cea076c80520075aaf70cd3f3275dc30fd6bfbceef1cffb12374046dd5fc7f0f4ffef62e5079021bb8818319c37b6bd05780074c58c21840868a15544950526b5bcb91b4340507399f9722afa387d925ec9f7eae4d2ac813ac34dfca217a1c73e3c209ace4fdca15cab7e28af9bbe802358f539633483df5b44b4bf6bfac5449b474c9a4fad35c20d516ee2c7e9dbd7b17e8de05fa2b237b4a8d9273aa95a2d7d9c23caf0b39a666dc9cdace6869175e7477a76804ce2a6f08d3acdae4d60dad9b73531893c191c2de9066c9e1bc81ccfdb8bcc58f32ac857efba1bf70ef02fd93227bb777ef7f1aad356d9c80e5963f42dee971585f7715b09dd77210c07ac46176ccbc8e3a40274e4b111a327d22bce4299c5268aeae0a650faf7d295a91e91a7b48399fb4c26c95fbd5b64d6f65db4b7791d636e47a974983825f53821a89f7243c32e6d112e7fb5e5af3ef2002f35bfbcfae0b82bfff0825f349df3daddd7d5acb98da397ff467d29a824feaec711eea97a4784d41c6a104bbebdc39a85f446b830a888716ed1c06ac6ebb4ed2c67ad24fb1d8fb497d0ff4cd3d2551433ddb97d843b91ce54fd50cb5afe6393b55d2f84dfad97a4269a4c34792740e64a963ca6f3ff4977f01adfdd663f7b4761768edd9336189c938039b160b501d2284c58142a4b2d65f446bf6090de0a29fb7c93c590cd34284168d4c10a4f628166a78a71838df6004faccbcd8958a7d66357368138655289201ce1248fd5256b466d92644bd569860f885ef57db0ebd956d6fdd455a9368130295ed5930d6238d389cabbd360eb78cc8f84e5a033e0a16824619f88ff77d8283197f9ea56d61764f6b779fd686ecab2acffe4c5af3bd36725dac33959e362cc04370cf9da0a3bea9f945b486de7ae89df5a58b4357d5e5a3631defda5e46ab2e5f170379b796f74957392b1b352eb7dd5bbf09f846060438cc6af4676cb94b7ca6cdc2bd2c10de23ad7bccd9403d3b27ff5b0dfdf55f406bbff5d83daddd01d37e2ac59cfefc65877d68856e236949b3ac452e4ed02f326d5564ca50df8e3e0ef14bdd850a4b01017d64ac29d2f1033ceeea4d62415cd30adeb603ae82ce8b856bd1a96e51a6cec4adb865cb42b7bdc42ce8768b92aea6575f592df856a6bd716fda77c7b4ff746724093659c53f6d3b632c7280248edb20bd50993beb1799f60e9445f611eef996b5ce4d8ca8ca79e17613ed9a1db6023ecb6a4530d16c39282dcd51c103aef74e56ecb08e974b502eee388646c471209dea8f95e6639f1d3d02de5ab06ddf3b23ffa43483dbc78f7f5a9a01d46a0064f5b959b11e642ec42e84642fe6107544514338cce7a7754f6b94395a2ccc8b479ae9da43ae42398f1163293e694f94a1478d953e04a98722618650d9fa6adbe5b7b2edc3bb9866f033636cacee537d89a1b7c49afb3483bf9f33f34b021985459bec548e7922ceab0a7825995c655290f98ebf2890c1cf88c84cf15cdd89f6d44c7407ddcc0cee306d57b4bc53535923f5197cb13d078efad5fb88584c60b81b8395f2741aac59d543fe4630aff9841832626928b8175f255fe5adb0e3e82e06326249822d184204f170aa74ed56dd33d4adbc7d42ffb4f87c90d8950c3d3beb7d20e37eec0300ecd96e270bd57c22f235e39b97c410e5d4bd5b33bf68ec5314aa8549d6d9d67385ed4d160bf889b567c86327d8127b469a5dc9bdb54726d33b0f909f61501721935677eaa73f897d43ce7e9444572b5ea18a38ab6e57fedbdc5cf5adc6fec95d1cfb08b89b32d9e4c6529aa6cb036bc7270e328eccaaf763ff1f3df64f7b5c3d90250bab5be9b5aa8ebf71f2b76cf51bc985647eff491b1b75a853eb207d3339f1f937daa17e8d6f7ffbd4b99fe6db2b58965f872526bdd7e372a73b42a6a09c8c921f6c173c9bf268b56d426cd14cb2a0a484eb7bbeaad60ecafeeef15b4fd8c08f632d55893e3219762e4426f2dcf4c05b6b98e37bdffe7e09c12ff1ed3705446b3a548aa89995dba5f686107ba5b078267174fde4a1eda1f4e53469292528e7ea1bf5ac34dc1158fdd226430ac23b4e43cea1744db461b222cab7c143be7274ccad6cfbcb5df4ed23c4ad11f8f23932373f7a5d15886650ee2173047f2f470bea84b90b1cdefbf6ff787dff2bd24affc2fc9bf968ce1c3c3e44677c5ec11078877d4b272382ede5b0f45e9ce259977c649c40d9543471543d266af732e111578e6499ed3646c49979ef34640da81ede859604e0ade5fde97d5ae97d5ae92f715d15f8e456a1b19bc05ef137a3422b470a8b5c7f307623b59cd6cf8cef977ab6d9a2ae9e2e125494424222252e83e2179957b1d620e74ece92882d3cd671e48d91f2d5b65b6f65db1777d175adc879016ba07b7a89b7ca4f5b282a74b6a6284b7befbaded3da9feeeddd3ef5eea7d1da306accda6e43eaa344388e1494e08d7a91e138924244fbd1c4c8edcf75413eabf85e71f17250abb25c169eb3602dea8ff1b35daf3a428a6bb85436a3794663686e3ff42fefbdbdbb92a4f0daf5ab557b80220cd780e876b95ab1ff98510353be09b6c01f32da7f7b169fd8b682a266a230fd0de28ffa5e63010023b140002810e44f44c23ff443c18878517fac3de4f047dabf3ee01c467199c18dbe62b662a73477790c3a39deb3f6e471074bd7fc404ae3378b51d5d23b3e5b1d0aaecbb5585c6ec64d1e957d9385d1afe88ddb47f93b5f1bb0947debfa665d24957c1c82850b5d6cc47d09f4cfdce3359e5daaf89fa19075bd5af57dcf6531ffd29868c9778c44b0889597ce0d56b407d482068eb11a0188c57d8123e7f40f08317078b05e204ed23c8507d776677fff023fafe479d97b11fb1649b99842cab3445bb9244ebc4fe187fa79ac9dc557fb9427687291fbfa861ac6a294d17c195d29c3003d3ba791db00aa3de85d0c31580e001de57290984b616f143f20b2f4ec21a98a7ea79602a9582a422df1bd0f31dc6bb15f4f58809f465898ef58c3e8c61f2345cb64a66121d794010023a240100010f4af222cd6dcf7017fd8fece79e087daf353cdbf0519cff40843269752591f3425ce719cbf9ae5d7e46b1584249eed59a0ce706c735485062128444c7cb028b2a1ca341fab064b7faa8ba4f4fa9d3d92bd91102210f7f45bd7a7871a7eae5db1ff9803298ef9ec09beb63574778a6779605e511b688fe613a37994353f3b82eaf64ae497263d7b9e52edb56d21c4809e84720a36019add230bb95fe5fcdf3edfeda739ffbd2bb8cd105f2612e12c6b53ca38d5cea3f7ce0f5af408d20bf482c4ea4e9a5c8501a268507586cc11882ff31ab660c412df5b5aa95f96615f987640a6a9634a7d25acd95b1116e45d74fe51f640ac00c2ceaee690546ad3d4a603c55ec208dfebfc9b81572472fee76f8f0e9bc1a5cb17d4a171effcdf3bff7fe554aff661500d8784373e42da7bc2943c49f3f6acf5ae82716232a56207b2239512f747a3b465a2f9c79ceeafd3bd2e937c7243840240c0101df59a739e062b6b9185016f3df4a1ee9dff7be7ffc7b55468a13ccf6a69e82c1e86bd54890cfb3200fada0d1c0004f9abb41408ef1b52c05fd8fedb39c2b165afdcfb2f2887a6dc9d813442d9b8079dca85f6d6f36f1f78c992f5de34a289aed2013baeea8d48265f67764a81e1e58c22edb1ed08bee8c6f9a074a674f9adebdb36eb144832393d1888618dc03ff4e20f3072930df6f5d32869acdddf8fe8f556ee92310eeaa5e164b25a25fd580a460ed2cfc921a0fe228a414ea7702f29e2f81fe0fc3711378243199f387ed0c20be2e997083f1ecfb5f742c4fd249601b3d4f9905c0b33ab94573da794056b6fdae699089bbc49784f04a9499abd00ec8d31c9c8abebaf5aeaf256800a7f179d7f00c04f3e882ba019f661245dfe3a57aca1cf08f6f76aa96f949dba77feff295aecef8e1d04eb1419b2b53217fcf970407cec17ba72cbd7be249b4a29bd8226b51f6b2471c9a490ecd095f5f976b3e41f878c7f421b956aa740ede79553499ef924307ef0558c3933dd0a3b307e1276c0fd4cecf8894510be3509fbade3bf9da4bdc78e7becf86bd72c222dbef15e8882b663ccf8400d12217cdca29cef6a1c5a2fb69756333e5632368638a2e1083a4465fb1a92ad9c5fb45f9080fef0430dfe052b5f10779a6ab3f23040cfce99ed56d88179177507422bb2cd19437708a68b9e8dae34e5360f4fc5f796bbfc5601957bddf1cfc08ebb561ab92d4f9dcb6616df0565ad440ac9d34bef05d57cdf1519768ffec5c6506a34262a0050b4663f625a6439e3a73e0785d722193394f2424952cbb9e5414450e4f5e4dbb28d26d0072e9cf3a174aa93f88f0a6b68163f3d5def71d7c6b1d337d7cc6f9f834c33f9d1d2c86b3fab3432d89f571a597bc34b0cf17742e820a7d2ae9e0f16d2019e4a2fe94eab05d03cd4fefd71d0337503581f3d0510690c7efd1f249e3df38f1e889ed7cad1ff04120bc9e6e103ffa63557db074770b3ad510a2758b118b39ac75fcb7475141ba681a77e9e41a5da6fdf7b4b3f998acd9bea2f387eb14b1252d76e8e20f36470f5ecd4a6b6bd0e45e34056f02bff88df8a7ff07e4120f1b71ebb0f247e435bfd92f9b1dbfb653f6d7e2c6ee25abbfe72788b5908532c7a9e9190b0f40d7d2d4ddd320384e967890d1d748acfb0ad8fb5ec7a560c290353f63190ddf94109c2deda57e62fadce7001928380b7b66ddcbb383ff649812e9d15038ac9fc336f3f02afec27b0d13741dfeb97c1cf579665ebfee7ae035afa7cf495aadefc416d743f3ff6cfd246d6a5d4604b8cbee05dc98ba867745a12335ac68a898f156c83fb9a173eeabb86020045ebb3dadb33317c1084cf44af94b9f09c53538582f8b39281bb09bd4f6c1252bb4654851f6e4b524a90aff93244b0c542856740a14736802575c6b94872061cc12260fda0365affbf591b9d8131ba7a02bb351a7ffba7068c18af2022f036bdcecf0a10ef6ae275c1b627dd1c29bfd87ed59fa406f831b3f837107c2d3defa5f2cebd7a01d6fbed8a6701a8c81005f2b786e8376eff0d0000fe318d01c9ffb8f40810f41b0e7099e51f034890d40fb50340bef1fb40f17f1040332a71c8fe6dd1f7c494d1bea16e51a2f37612e80ad3a361fb5c4dc13f19a4c15fea191b9bfd39f22d7269c69ddae6b4dbbe4394ae78ee3af772262f9de08f3d8235d96f5d144c447cda79b2d9aa716b9565c178c7a13a33d1f2ea521e84364c44389b57ba010fa3c7cca0205b9438e05226d0f61aecea93b73c43e753b7b0101e428b866ab15b7870bc15410a0f5d620de20dc365b6d32271c2579066c9af5a93b066a859d4dfe4dbab191c988a22d9a26643335e284817b43391b889c064f6b259f66d5902133fecbec5be2312f1f66b037f9a448cc6a27ba4864ae8d7377c237a7ec5121d2b682bcef9d9ca6477fee3890f1aaeb278ef2851b8a21da6cd89576c937df0c5e6cec583f3c9ac65c9c65007a11e3e9baf1251f6561291f82e4ac4211d4c34cb23d2abe7f632d59d665972c6124763df2b11bfb1ac18115228eb3a372621fe5e22de87debf67ede44f0bbdfbb5548fd559a176ae4c954019c98a22e72174378dacd0460caa4c687176a853993d79aa76b92c903e9e73ddc928d4b3b2ad7995d1759601ad2e84bc4cfd62f90140cfced9fa56639feaef5e2e101693aa794bf23ef4fe8fc78e5f5152e02f4cbf24d4d7909d48ff6c5e44ae4ceb047bec171dcf897220c59c9d2c94999e648a328cb215aee065c6db205b3985d6c41c8e1f56e469a576861ae7f2243b5900e2432ce0d6b281e4bea4c03f37fdf2c78cf65f5f1cce03c5231cc5a75ab7c6327b0f7c140130d075207f61fa2500b9ce5afcaf6cfffa80bd5b72f6e415b9e3d91392cd06d9d454fa7632aa56c541df8b68ce847fe93143c67fb2eefb34a55c35fda58cc2dbeb8ca1e62fb2546f4cddb96fde5cb9a5e8f0c77debfa6f0272d434fab1526c95442059fc8f9249f13a4abaf286f1f6b0cc6d74664d7125d119e831a47d2dcc939be5d0c1eb0c7cb4d249d0c01e3684762fbaa7a5a51dfd0352a8f2ec2fc91f9b91e364711a256bc6e0919fd594443c327c9f7822b55c980dd75d26b23082dc9b39ae110f2140135147f36854ea0b9350a1b982937f8691a06a8bf55740cdba15a0b2df452d46638fdb6031f2793ba38bc410522c88d0c9bbd5e27bb5d8fd0ed1f75aec0e86faeb6c464d39baa9c0ad25e0746e92c00182978ed28f5fb2bc8cedd1f6c3c8dfc807008a363e60708dae396d42cc0c2be4cce11a6f90510491c3e01a77d13f492ed354519ab310cfc6a041b7b557e1e3dd468ab445d3f77b6105462cfc7aabc7f513c20dbee58f86fa37fe9c50ffaa4d6d2699c1440e7e7c2b249a1e361f610594fdf786fa1101ec4f1b56f4e87e2fd4ff37e78a73270b48c76d8064827ba3c4c1c4ec1c6a5742c90d39919aee4493afe522538c657f975e4aa6b7ae55baad2893a2254d5aa5572293a1db4da2834749c1a25b2c50cfce79ff565cc17717b9e2da0c803bb3b58fb839305a8b4ba954f8710b94e57bb9e23e55ff9e2bfe11e9b692356be11b13a6c709103cd72c394e1632151b7ddc94d1bb0fc38f1013be204483f946abb6e80ca81b1f952126be5d93e9a262b04613a7be5c2766eef1d83d9cfe8a1da7b7c20ea1bb881d3f3355ff1b2921f7d8718f1d7f0fddd14b89f7aa73a212e9bde662bcb978fa410f3f7ebee5c9bc4d30dada38a1ef3008c676bf603e527a877aee6a28d6433421e1ec408fa09b293ae9cf06a02835ac73003d3b17a45b61c78bbb881dc4605191be5d89e19131d32f812f61586749eb72ee75c73d76dcfd2d466e3f61f0d3f20c48029be35f241cefa2c9ac1b2964197b0f5ab80a9c55f7b3b9bf0361442da3aa8575e117730d93225d0c8d9391f25d4576481ea3525eeb92e6b8f0eabe26caa702ead9b910de0a3bc4ee629e01198e09be4a9116e5f380e8d734d4aa0e8c69a22ffe24ec8006538bd68b39a26cf9c1b17f9f6770bffdff774dbbfcbf7385089f5f15a5abee3c17238f642e2a036b01303c50ba46a5a51cef7f5bd9d18fbc47e618552e4f7a7c8a337ec61cc9604f14725545d7663da112252a909d8fed2507b8f5d017bf2fd5f24fda95e32f4ca1c9b24cd8df9bab8109fd6c06cef33aa186a8039bbd8fe7a544ef92142586e3f34a1309914b735cf738e6eb3001e9eed8345ea6277af6431ffae439c22eabea43eabfd21ae9ad6c5bf22e4a625fdd86221130840f00e92d2bbd97e7aad099890cf7eef43d2dde87e2fe003b70a193dbbb54fd89a9d64a76c8adfc6b16d9e7db56e28a63088dfc427a69665c24eb74666748fc29b538bd76e34cd42f6734f3d66dec07f480f804d59e1be5765f79f17639348a77113ba06494a14b1432693fe2079958bd080f992994aabcc78e7becb8fbd871fbbd0f7e5ea9eefe6658a141e4e0b5e2833c14f9ec9c42c6e9e867a8cbbe3043ebba48b924470e4f9f3a5bc2235a9619669c426e54afe3d38418849684898a006f16a29d411bbf6207dfadb043fe1755dc01bd0d764419bb3d5bae24535b7c1dce21f412213cdf58fae05fdbe7e521d044f25fe8e896610d6c6534880dbf3c6dffe391ede9fa9d5d036e9a9f7c10d7a4fde81e3beeb1e34e87f11bd45ab4a9da5fe09d2fb7c63c611e9f513bc67200419011cd2e26b2606543efa0482faad97c9ae6d2d202490e6f31377e40bf22b0d0a8be69128b4fa1d8870bf5153b5edd0a3bd4eea2ee50336ca5e410c681eb9f083cf7a7845a6c10d462fa4edd715fe6ff1e3b7e2176dc3e8cffd3b0430517d2e188588d2c403d4d52139f0ae3513323a411d75bd337721bf6247149c44f82835845dfb0c23b2f043f8b4a9f57e3dd2247cee120f293996327f4e5a5befa8a1da6b7c20ef5bb881dea259a0a60e509c30ff9f6df1456021dc8733e7fef1621df55e9ef1e3bfe8ed3007fb3b2d6ee607f65596bbee671d7bfb0fdb77350a1b33b15b39e3b4522dadec098bd8f916bfee04aab634d8c9628a67f6cb0434b906f23a8fbf66909047e69fabcc8626424ab3ea8cd21d3472dcf638d11af2a8a6f5d5fab39e75232dae5d346456c1cd706093b7d23ba0148e1a99615170e2ee6a6503ce765bdeeeef4d1047a09249c652bf87efdb2f7b3528327f8e2556a6148b520957f89d1017f9ed11588c4b4be17e01743c7cdc8b485aad2010021494101207fddbe34e0aab2743fd61e4ffa23ed5f1f30e44acf20a75605298ee84db5af8445d5f9591a7e11eea1e93e64687b3aea03edfdaa1396c9322623b119864d23e32ca90fd2c91cfdb1437864b2754a8c92b063dfbabe69b2936b66a93c521774649ad32731ef7c5168e53c11f7ea59826c785ff5332e2ab15924e4948fa4376e937de9af5e1dda20e03612aae35bf9575bcfa6756ce737f6fb280eaaa369bb02bcd084add700ae67c0c7ae1e88fc4f6a80c478412e1dff7a33e2882cf1d4b277cf8b73f7bca09c6ddee33ff77e721ba5f43bc7fcefbf6de573bd381ab6fa4853fef7db4d2065668923b6c0f0d36f63a0ffab29301ca6d0f57b3f8f70d496a9cf8804f8fb972d93ed185738deca74c2d1a46dc17b4900c894b0208dbc626e61161a93a8b57b0daefb39fa8ba1224ecd2c23651dde5b71680f932596fad2f02f5f328fcd8afb3cb5807a762e8eb712993a773157446dfecd976dcdac545aa803d0d229d1bc687e81ef2d097b5f93e26f23127f41aec85f588db389b36e91187addef3497194d1d451493e280243ea609b352d8477d2f2fb0fc9512b3f89ba20d6b0ad424f88b47ba001ede81a1a35718733d0a9790478d99b184b71ffabaf7b922f7ebca7f5c8afedeba72f0bf745d79c709c45fd9fef501b70ebb61bd6918eab07a1fadbda92396cb18f540c00bdffb442b9cb52495e11dbc7f5fcc86d6ae9f3a5a4542660d739205f988d7a02c59c726c24c9a7e9ebd18e6b7ae4faf19efe91bfda2197b528275ec323b867f4ae30330f200c12dbb37ed5a04394caac4ec662b487445c64ee95575a2d7f001928a6ffc7a39549b2622bb418b18f73f2041e9b9b2c01a147e0b4b1a824a4bcbd0a974be4d9b77a33dc6638020142928251bf26b088ed27770c9c241470f203c1a62b2626350a6d340a06e90092260a0979cac017a762ebeb7025483bbb835c7b7260affec24837fed9affb244ec7dc0eeff022d364ca75ea4fb37cddb5d3062a57b36b152436aae79f308f59d347625b83adf934be2a1b721c72db39cc500bd654822c2225df08e1c63f21c83e696674b597daff53d6b70a735832f1ce3bf4247e0ada0e3e52fd062bff5d8bd16fbb669ffe9e5ab6e9f3ef3d34c7b977f07fe267d69f7a970eaee3e95ac5cccd8f386ba0fa0c2ef56fc4c137d3ca8618fd49eb3c8b872cc51d73b88b0956e61cfdbd3b77011306f0ec7c76927f4f07d75336e576dc5ecbe7cd53fa9e8ff5fb8d24aea14b65b5fa21c46fb0d57142c1cd797e7898c0f8cc064ce2fc15dfb01b93e8f2d092611f39e54aa61ce55078a1a4aac37e4730eeebc797d13254967fbc13c54f1ab6da7dccab68def62f4ac19bfd8298c2379bcd1e5003527bb0ce9084449e03b15dfb736547a8283fd46b7950fe87c1f3dbb9f62fd5f1609fc792106e87a97da190e5fc4b74a2b41c62fa65d0140803f2808e80fdde287420c8f0241d57fa8fd2dd0e10fdb63d1e6be35e43ba71eeb840b0d33862ff1856521640b6b79e8dc6063b93a52a5ef59ba5ec2b747f54e0b653c3a5e1c06489478c7510e41c1c21014b94bac0cc036b1e2f23c92f9d6f5291231d1a976b86c2a8e1c77decf730af5b3a92ab30c22ac90628b9bd110369342c77a125c5332b62cda87a736e7ee3832b34ec52a325b0c3be62529529a1369ff2ac2792ac59cfefc65877d68856e236949b3ac452e4ed02f229c7a642635a351eed3d766674baf0aec6545282195de924b28e1efba19f9d1d14809eeeb0df36ebf35db328e501023b6fc8251ca0301751d454904ab8932626e2a00b8b59832bd8b843336ace3cf88ebb73f6a9d0aff2999a7e3522f76ff9e70ee43047ffa748defb591eb629da9f4b461011e827bee041df54dcd2ff2a34cd3635eae282c094d5becd0e2236a8f3c636b3488c4c8617db0a89f77da6e93606add05c4b272a3ece88d0bfec4c3ddafe0c7800cf54a141d66ecd1a47b920524e0d65ad3e47ebae69fe44725c126abf8a76d678c450e90c4711ba4172a7367fd225a3b89261362878e2c0cae9b34aa937d4ac18fd2a423154b4cf5d43abc718a24588a9c489993e7b0f646682fd5b963de2091035e295e8e09c249adffc908a1ac38eb57db9eb8956d5bdd455af32f78627423fff4d4c7f15507238b3e6b6b6302dc9f143907055a26aba71b9609ddd3dadda7b53f7d4fd0c2a24d762ac73c11e75505bc924cae3229c87cc75f446bbabbad0781edf1732ca3e7e4b49a7a4bb8e984f30a6c28a6c3bd43242ad546ea8604546fbe00721951c4fce2f035c6e97490921144a7b9cb671d5a672f2f626480b71efad6f77b82de0d5a3b65f705ea08daaaf903009e4a2f69ff672fdd0cfcc5897ffb4dbfb7675de31fe4317e3edd2f128f53a57e5dcb7ab3aa6b6201bc31e1bbcd58f89de33fe43102aa5e6bf11e77e99f46fc87f6a12e40ccaa82faed420fffabe987f218e150668b16ca1b017fff05fee17a1de8da4238beb1b3d0d5d804f66971215351c73a7e127af8224542417d47b0e92200f4d31495a564ac9a57f2f107e17ddc6d00f389cf3e9c49f05774129d5f6164f95630e2f293e6dee1bff5136fa320be551ce4968b746133382dbfcf7841905d91620f59b13ec9d87de3f33f3af70e72af40fe06fbebfc8573ef6d7190e99d2f5d3af80dc3ea0d60ded28771e741b2cd45888f186e59e1e94bb28a034a92c14b34bc064012678983499cc1e2d01d3b5392b289b06a1522a0ebbbbf42c7edcaf4badf4f50de4f50fe12c75a405047dc25fa4514c47a099c74de53b63669debdc798d54d17ef23c84c5eaeaca18792608380dbd7783c7f2b0d9c176770f0c971237c5094f3847a87342bb404e2f6b6ed76171d6bd236c3457a9001dd6873ab2fc426782e18dcc6ccdfeb587f33bd5f182a103cbe45ebdeb1be5f3ffe97a6a3d24a757303d095480cb53e988f0a5eb68862eb8ebb3d099e3dc6e8602f5ea421d03a61613bec9aaf9035dd48c8e3310de8e6eb1893056efa1a2661bd57328206ff3af6cf6f35f63d7ed1faf1bf521203003151b7aa5b73bf7efcef2989fff474d4dbaf0afc6992d8f0532fb15fa41889bb9bd2d30fcdea2cf1c1a5de0092b1ca2c5b667565deb561297719b20aafddf9cccdc1453aceba07bcbe2a88552aac8dc773db23b2be8f47be42c7e75b4187eb7d3aea9d90c4776d072fe4483a42a9f7112fd8a088e3d716509dfb8c545fbd29daebd3e36f08607b831e0500147d41225b7a1b8c6082913a4228805b62f25ea5ee91d8a7ee1cfb3a5aebdaa2f99b63be35ef18cbade244eb1710d3a257eef963b6a0ad9cb0b5212f609c44f962c71e637f7f67ffb683d7973f6707afdf0b7cfe6bfb6d76f0026a6f788921fe8e57f1f72fdf573c9830e0a5a4f1e6494b6ae5d6519fde70bd6a282308912f745910cd45d76c352d2fb10c3ee67a3d4a690cbc663ffb0607d429edce60609e08b21f26edfae756a09e9d2bddadb0ccff2e96d1f9992ed0fd6e8ff732e8d7b950b7cfb9f969d8a1a34bfc311e9ad3f773ff4b36d1546f7e42ed70cd376b203b725d503b05fc931496b5ec0b1a349413c58844616fca1c0fabcd1ffb6837465ca2356535cd197d5605e8d9b912df0a3bfceea20bf5adb2c1b776a1444578eeb1e31e3bfe7cecb87daaee4fc30e3601243f58c4a5c0c70fd9a43e04a35ed1ae50296112b23ca6c53be87ff8f9833d29f59c473b186a20881d4f8a98c79e45fa064754b5b877dc54cd6b8fedac17945fb183e956d8117017750735125cacf79934116d7487bdc723d3eb2e772bbfefd51df7e5fbeeb1e3371ff53f267f84a6b0db3987efb7bf9e25f944bcc2c8a91e1257761bbcf89de33f247f80686b2f2a12fd3fec5d0938946bd89e6fc698b16bb4c99abd48652b922c75a843a2ec4bb264a490ad2c6519e46419db942c5922263144844ed94a268c752c638d2c2912b295fecb39a7ff72facfc93f746a8e33aeebf3cdccfbcdfbcd35f3bccffd2cf7f3bcf978fdbf6138806d81cc23e5db756117402bf6915747fe60c2ed5fcf792011f42fef76782a34c5b297281a294c3bd4a5cc8d1f02d11fd5a75d5db38fd595e280d0a933ffec7866cf721af64aaa443201367d7bbf0dfa4cb4b519896e8f91c65d4d214d4678ab9235a7cfd342144cc25c6abf88fe548cef63fa0c6ca3e19d8a281eae088170926a9d5c8fcd72f3cbd9be88870e4ce1371376f243f99b16f8dac36394dbc485e7eac65446f326c238dabadf9d6dc71ddc9ae3cb830bf707c9ec636c2d684be04e0ae2ed03bf372bf9b7773b3c7fccfa3648c9426ac625ad57969fdd1744cf31460b0220e01f2574d0294ccad7c71f48ac6ebce6e27242a13738b243a755a61ab07b7a2777770adfcb055add11698e5acf99b47488e7d4ab1d4e1c0a0fd9b66c4cb5bd2ad6f401dada7f26f0be6ad7a11608d71111db2d6f8c979b9f272d267ca3163b1155efd4d07cbe12410a1f46442a849e18d6b8e52c382a84b6615059f824aa33ce002e82dc92af98dee3a2fc84ef74ce259b7b19eef940e3999259f0d9df81016ce9700d2ab6cad865226875c1cdaf66b8adfc07b4ffce8294f069e77d653a8e2147b82e419b9d4fdc68ad46ce1dbd4f6a329952883bc0b374dc21fb9144f7e61cf04e411a49f0f9b7a9d95219cbadc7e5380802b5a8837bfe94b0d8afdc6d39ad7c27203bbae5e980a595c8d008f45b2d4872bf5f1008a4c49a2e3dafb99f44a0233804f4a7ed8af437a853af91866e7cd16b556cefdade62c28fc1fbb2ea0fe80e89b64de15e2bc85bcbe75611e934c4a577a99f70e87ec9b19cc0aef9ed0b38223cb5d3536fc0dc90829a8665da26595d297303427b3d32927483630f22d54c0a8fe534b262fb7836d3efdceef834ee95f94106bdfec2602d35a30d46eedd3d8b3ecc11b27c98084aec68744330d451027d8887abc0d26dd037c5d425e04ad84a7d9895ec42bce48f8ec6d4f68a4c58fa39aa0ff32ff761282bcf16d7f7ec43f8245178cc8fcf5dcaa843f4a04c6a372936f5b69669df78dc0e943f08841b6f53e15eef2d6b40247e501f09c5a764abcd4f059eb105540219ba6dbd87dfa6f1f57b42bbda09088e048fa040168f33f869597fc5ba6d7ece9d7104f729fce86af36ce3d43c1b0563858eb9ec9eb82e963d742ef20a5cd0fc97659c633a69b3c4905f87ae9f62afdc3ac85ada2c30ab4337909c357d3a0d5e53ce8ce0977c887d72b61e0b93e74a1e8a5ac40af2da89c65062bceb1bf642a16eb147c50a4ac48a13e249e8138372e217225d2b2b771e7ac23d9d35f6b3694f3f27adf882cf06e75e1008f74e04abb3a03a7a26dea65238fa498c89f7c79e31f8d0d45dde2cb68b6843551682ab408b6473bd040dc3c921f5a804febed7aa36ccb14d9751f59599d99a49955a1cabc48a77ff0c560c5e284a15b66e4de78dad806db2e254e6bb0f775f2956b0732a8bd9559636fe0556805b0320a2d9dae8aa35da7a5ac8e5b198d1d4894fb373153ccda693270c9fd5881870397bd2655a8b2a0c0a9a5db4bb1575645d60e99b698797ce77f8cd3ec8b73ce4d52dbf7bb8cf957528f7e346806cac88fde7f965bfff62547ed9577519b83ec07082d55d1184627dd68362c5b77affcea6ccf4c70589ad5136e5821420f66ba14ebddbd05d49f4752ca8f4a2c935b7ee1d2a473a6ec5e714244d388d091d1b727cee708f6b4be5e484712749d0af7fa3bf532cd37358c9cc04afffa2b4679125eda9df814df9db2f4695f6af4bfbf7b1fac92f9efb6656ff3dccb65885a62061ee4e93e88c6303bb2edd41a9d1c6ef7e79f1c4bd92b68d1708cea755d9aa99ed64dce9fb07c35f247116bece3b82438cee0aea3da8dae157ceb428db4564c9f66d4ab4fad9c6c12e20b5cbde0e305113d20ebbba6c7f35e6955afd54761dd5eaff4f1427f225cb4f5837563072bce7954a9dd9e495bf935bb47d9c94649f62eaba07a779789ff556bbf485d7fbce0c865da60d11d9c9ed301819252ba2c3675173fb49a15027f9b8984289c589d7e3ab1d7a100c11ee0f18aa73dea81d73425f945ba9eea01627ae91b5bfd6fb75046eb635981778d88c55123629e8eee18f47d5ed1cf4493fd914ebb2302fe93ca0a4a83d3f5920535e901d2234c9b8f75d9edecb732af6d6e2d57d46a4a26c3e81c5b54f5ebf8e0c4ab41b84b9cef11ae14e6d3f1c74fdbcf80e630fc964f523df33b344b51bd6a0ee58e3c589aac223f67bb474b8f2d4bdb2deec3d1a1191abe6973b9fe8976c6c9893c7a8fc5cfa02732a21c766ea74e85153ebd3ce63f9b3cf4ab6da8b6e418b5c706655f72b03acdc7cc064a90e1cb53891ea4e7f17581c559055aa6d13712bd6e8d4ac15cf2b56986d0abe148994f74d993e8b1323d4acbb7160b0ebda90cfa647878d8adfa40cc4771caf98191f724e38d39569cba6c109b272f311224bb6f3281116e3d6e9398bbd4fa535d459089f9a3ad90bbd6e8f5c212c02893a364cf10fa8b04885c535b6e73b0af223f77c57ba2219fd03c77fbb46050b5854ae872579e9a32ace2467e56beb7dd087bf0bd966a71b783cd7a9ef645ac185d39df446aae9e5f5a63e133e4ad75effdc02c3773404290a7e3aa9c2d9b0dcfc7491c68deac77cf0f4f06d41e18f6f6eb776486aeb7391e2679e1116a40f3aedb89edb499421244f577d34acb375e846a6f43a63b30db9e3de565b1e2fd4234359be1760911fc3f96680d534d1716521b5ffbedaa45a96c14fc1d85a9de12abf3757cd7f91b6afec92638814d0d3259c5208f74c49bd565eb05f0dde25ebf2ea2a4bf05078aef50e16076f8e45c012210bb0ee532243f05bb23e5652e5b4541898c33ee0c3f4e6f054c0a2c67fff0947ee9bc57f3b8a729d16424917223bceae6f78cfe619d79c592415462fb8496fcf0de362050df37b720a282d5b526959c847654274ee1bdc36e688f94ff42c07ef56ee3aec7d6751771c244b77145062fc573dac9cbba845c32d3d8ec65ea199f686c980b6d63fa43bfebfba811afffd2f747dff8104a0d45775890fd86ddf0e565bbd47578f1e6c759533de25f9e445bdcbe5fc7eafba0387f59fbe341f6753bb44c427c4ce8463fabc8aaa671b59df9ca64b3f82db6ed14bfed27f40edfa4e6d30f517644642fa3aed43aa9868027c2bef7e9679d724ef0cbd175ba5acc29da34fa6ec4df300817093d7f5a2c29f31376f36c73ed0a29b2db7d98395ba64ced9de4a30adb6388cbc73eb9492a342c3de2dd24934f2cfca042cd7df3a7025cd059a13b7cd6ce65e3fcc4770b564c6492af19d825da0fed0c699f6089474e436d9d09844c5a2a719fa1f24841af2f08e9a28175144997db2a406af4eb87cb113b3dda6bccc21ff861897d26ed45d654f364b42f36ee2a22e33264b97155162cceea2f3d0473d57cd4fd6f601a3e730e65a818260e84acd9853020bc3910fa931bbffb819446958e1b2e3b1c20671effd7c06d50f72f43d3ded066fd2dace38fba59b68ada32b50e20481705309ec014a776f5bc87f3c32af7b5cea6607e72e85f5d18dd28ac25849521a3bafd2f66a9cf23a45ab06f84bae4c61a4373643faf415af1c2636d35c3efa0f25f8bed516494d513ef11dcef853dd0b1a76cebfc08ab5eef27227e14f33839b430e06cb4f54489e6a19b9da912ffce89ced82f46eddf241954813ef43f5386d5575db990264c1861a835958bbb98b34327a2898fe5e6369e1cd45ac208ff85e42892e6fae1794679b543d5e517bfb78a8144acdfdac98de4ab16219da031d8de93df98ac1ddb7a82e2f759b911fe9f276f7bc9611907e9ff11457a07ed79f91cb3efe4a61c5a63edfb0c9e367edb08c9267124a9c8e15ce2ae808288c76d554489d0bbf81f6e295a8b2eff2aa33ecdfaa41fed22fa56e3342b9352f6bbd1c785f6864652b8fb86cc1430f82ab5418b771d3d9b7366906d95530d75183b60133fecb818a3aeea3c884c719135e9bd6ddd442cf26bb7f941bbb56dd3599f75cffe74571ef224bdc0994e8155943e7448feba1888d532cbeb66649126a0ed818aa5744454a2a9361299301fa23990c8a2cbbdffdc0f1dfae11551c766fe5b866602733830494a6d66589f20c17cfb5f4a30cc409977fb90e1bedd07da8c1eb2eedc1905f9d9ef74b316128bc67f0f0b4e68833a986993b336fb9f98d25e8c2ef696c8dad4c145c70bacdcb76dc5b4eee8a35a9e4a71bfc73fb372dc4b8886b7a66453fc36e8ef6e75595b3e679b297f4b6df933730243fa1fea05156d47f80913ebfdb3512c75af0de17e22ea39b11366fe5caee4c7b8b47c657dcefb08526e625f7517ff680bc8db10cc4315d15712b54e25b49b6f009bc6e68a5d8b99cf3fd4a80959b0f797b5ed45322607dc76c2415b0a861bc1f11c6abab3d5e04eb157e117f220df3e8553e1bfcb1d7f6f4fb47026e7abc4f50aed67f0c02e1a611082f97055e171e9c9ecedd2c3473e6ae2ddd61263a71a72c259466ebb6c6ebd17aa5079fe2ae8e34706683d9aa44394eec090d8af539b5b718f73ab8f4cd64f46ac378d3d4940f05a77cc6845b264ea61d9f3bea68f653d759d31c92be28b1bb23e1b804435ae326d87096c3eb9748ecf54eb7a72e673b86594c09e6bcdda3c906ebfd1374f721e3dc23772e62c52bb2b0a28112b18207e5e8f924384ab25168bde4c99c2dcd1fb53734ac102ba0b695a63b2c58a958f19f776ed67a0a0036d24af2f6774b8605dfd36bf6d68f816ee7bac62e8d955755d94c93eae98060ac0dcd7572ea147b8e22e6d94ee4cf04b0ea56a35a6a23f66d7a5f40c31ff31b6396bc6d191b2931053065494fd0e98bc796745a33b91b1a96fb9c9cd359a99db97cd5f3652b35d18f56d4140065affd19cf40c0e2d04593ab20903ff8848451faefff1f58226605377cf5bdb4766fc83173febcb23d8ec27a15ffc840a4b48fce6d58a31908f99a3c0ea2c38323b873ea3b1c13727220c3b5775bb626659f7bd4eb40c315316463ce0932283e1f2b833513876d407398d3fc8a7801d7dced1171e75548ef502840b6e669fa0e1988df7e316a06821250f507f612f1eeb5d800c9e101c55a1015232c1fbf03428d9a461aecbd664364a363edd0196cbbfd3c0ef16a7ea0737b5879e72846a776c0d24a9be6043a900551f5cab4c06951b63f9025dbcd9488aadfb270b26a3a4424f2efad228b5debd2e35cc6f8205454a5a60bbe904800f4cdd205bda9977d1b62a0d74d6c80ed8611e1e740804ffe0fddfe851952d2baaa71daadb95f1fdf1fb6dc928f3ac4d7c376e5bcc701f4bc7ff0639ca17732ff6c9fcc1497e18332630cff9c2ca39c155d993ce4668f6cc1f4ba0be2ddbb8b1598261967f3443926ae3dd157775c6e7e4b7578117d169d61af50f6a503a466c205b17dc06b83f31aca8697ce6bcb1db2fa79c460cbb6a0366f48bff2d1c85a7523cfb4d2c82aef71429045a7a8856bc9faefd4d391fc42fd6f664ba1314c2ff2a52f8cae3fd3a5ce620e4d1d73d8fbcea044c02c763c3db00f967b566bd349ae61fbbdfef3db66aa6acd9e6f79c71e55a94aa347523190db91c96b55483ede10a93d1d29c2962a4134c0c7c09518a015d9082f45bc05e3214122914c0e16658812a858f27de626f8330cf01c320e0e1629ec14aac2005d85e2e58866f84cb7201e033cc700158836e42b2ced134435066847f640ab21efc0447851a7500d0620216b31c053040103b4c06b2013e04a4488481d06688577178ab797098d5d85d61702f518e01962b65b70125c0b09156983f7148a9f8f62b22c85ce58341402550802a41d3e056ec0001d48b40886c9b1b110e82d1427950911d090460cd0896cc2005d483c8288a56bc600751012fc39828801ea211d65421df0160c508de84676c27b90ad18a006d182a56bc3009d6542b5885e6417bc1d03cc5a1010ddf0e24ea106480fa11928b99603fe7c7e16c3b8f4e9d2f3e763f1f9e7e3f608f35f5d9a58c6bdf8f08fd392573e1ff838c68079a6cf6fc1c731be8f63fe3cb6e4f247718c4bdfb5f4365f1e8965dc8965dc8129405c18cd17b32cfde04b5efa7c9e9da76b47d37c39f7e263fc9f6f1f1746f3fb5dd26be9fef8d88b27621ce3e2c58129ffe7dbf9e2a65f4c4b8cfbdfaf3a3065e9754b6e4b1ca15f32c9e2c34f9ffe070000ffff01";
        let batcher_transaction =
            BatcherTransaction::new(&hex::decode(batcher_tx_data).unwrap(), 10254359).unwrap();

        let mut pending_channel = PendingChannel::new(batcher_transaction.frames[0].clone());

        for frame in batcher_transaction.frames[1..].iter() {
            pending_channel.push_frame(frame.clone())
        }

        let channel = Channel::from(pending_channel);
        let d = Decoder::new(channel.data.as_slice()).unwrap();

        let mut vec = Vec::new();
        for b in d.bytes() {
            if let Ok(b) = b {
                vec.push(b);
            } else {
                break;
            }
        }

        let raw = vec.as_slice();
        let batch_rlp = Rlp::new(raw);
        let batch_data: Vec<u8> = batch_rlp.as_val().unwrap();

        let version = batch_data[0];
        assert_eq!(version, 1);

        let batch = SpanBatch::decode(&batch_data[1..], 0).unwrap();

        assert_eq!(
            batch.transactions.len(),
            batch.block_tx_counts.iter().sum::<u64>() as usize
        );

        assert_eq!(batch.l1_inclusion_block, 0);
    }
}
