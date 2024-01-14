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
    pub fn decode(data: &[u8], l1_inclusion_block: u64, chain_id: u64) -> Result<Self> {
        let (rel_timestamp, data) = unsigned_varint::decode::u64(data)?;
        let (l1_origin_num, data) = unsigned_varint::decode::u64(data)?;
        let (parent_check, data) = take_data(data, 20);
        let (l1_origin_check, data) = take_data(data, 20);
        let (block_count, data) = unsigned_varint::decode::u64(data)?;
        let (origin_bits, data) = decode_bitlist(data, block_count);
        let (block_tx_counts, data) = decode_block_tx_counts(data, block_count)?;

        let total_txs = block_tx_counts.iter().sum();
        let (transactions, _) = decode_transactions(chain_id, data, total_txs)?;

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
        let init_epoch_num = self.l1_origin_num
            - self
                .origin_bits
                .iter()
                .map(|b| if *b { 1 } else { 0 })
                .sum::<u64>();

        let mut inputs = Vec::new();
        let mut epoch_num = init_epoch_num;
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

    pub fn start_epoch_num(&self) -> u64 {
        self.l1_origin_num
            - self
                .origin_bits
                .iter()
                .map(|b| if *b { 1 } else { 0 })
                .sum::<u64>()
            + if self.origin_bits[0] { 1 } else { 0 }
    }
}

fn take_data(data: &[u8], length: usize) -> (&[u8], &[u8]) {
    (&data[0..length], &data[length..])
}

fn decode_bitlist(data: &[u8], len: u64) -> (Vec<bool>, &[u8]) {
    let mut bitlist = Vec::new();

    let len_up = (len + 7) / 8;
    let (bytes, data) = take_data(data, len_up as usize);

    for byte in bytes.iter().rev() {
        for i in 0..8 {
            let bit = (byte >> i) & 1 == 1;
            bitlist.push(bit);
        }
    }

    let bitlist = bitlist[..len as usize].to_vec();

    (bitlist, data)
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
    let mut tos_i = 0;

    for i in 0..tx_count as usize {
        let mut encoder = RlpStream::new();
        encoder.begin_unbounded_list();

        match &tx_datas[i] {
            TxData::Legacy {
                value,
                gas_price,
                data,
            } => {
                encoder.append(&tx_nonces[i]);
                encoder.append(gas_price);
                encoder.append(&tx_gas_limits[i]);

                if contract_creation_bits[i] {
                    encoder.append(&"");
                } else {
                    encoder.append(&tos[tos_i]);
                    tos_i += 1;
                }

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

                encoder.finalize_unbounded_list();
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
                encoder.append(&chain_id);
                encoder.append(&tx_nonces[i]);
                encoder.append(gas_price);
                encoder.append(&tx_gas_limits[i]);

                if contract_creation_bits[i] {
                    encoder.append(&"");
                } else {
                    encoder.append(&tos[tos_i]);
                    tos_i += 1;
                }

                encoder.append(value);
                encoder.append(&data.to_vec());
                encoder.append(access_list);

                let parity = if y_parity_bits[i] { 1u64 } else { 0u64 };
                encoder.append(&parity);
                encoder.append(&signatures[i].0);
                encoder.append(&signatures[i].1);

                encoder.finalize_unbounded_list();
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
                encoder.append(&chain_id);
                encoder.append(&tx_nonces[i]);
                encoder.append(max_priority_fee);
                encoder.append(max_fee);
                encoder.append(&tx_gas_limits[i]);

                if contract_creation_bits[i] {
                    encoder.append(&"");
                } else {
                    encoder.append(&tos[tos_i]);
                    tos_i += 1;
                }

                encoder.append(value);
                encoder.append(&data.to_vec());
                encoder.append(access_list);

                let parity = if y_parity_bits[i] { 1u64 } else { 0u64 };

                encoder.append(&parity);
                encoder.append(&signatures[i].0);
                encoder.append(&signatures[i].1);

                encoder.finalize_unbounded_list();
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
    use std::io::Read;

    use ethers::{
        types::H256,
        utils::{keccak256, rlp::Rlp},
    };
    use libflate::zlib::Decoder;

    use crate::{
        config::ChainConfig,
        derive::stages::{
            batcher_transactions::BatcherTransaction,
            channels::{Channel, PendingChannel},
            span_batch::SpanBatch,
        },
    };

    #[test]
    fn test_decode() {
        let batcher_tx_data = "00656531d7fca1ad32740ea3adca85922a0000000005dc78dadac9f58b71c9d7edacb77bd6323dd823c8ffeb44c059dee7ffb405f9b68b2feb9a3ef3508cc78be9f9edab1ea8557c09e3b1e83cffc05f2a8445c09141c08145c0914580010e181930012332c588a68c114323238c603cffb8e3e20ecb8f4f0d365a15b4ffe09abf6ddad1b7755a79ac67ff39b7bb9ddf3c67ab929e46cd439bf56c7757a8f67dddd968dbf1fc647b4498f6929c0b75a5f2d5557d491b6293a37343b33f681e2c37ae551763b8fc8c598271c67aed7426ff8e2dd7170a31ffbdfce97bb5d9ed0b1dfb94efcb6eb5efdb1bfb7152f8c4b9ae321c5b73af7f12517f3ec15e6effd5f0ddae251cd7673eb65b5d26a1b1e5e68e4b328587b5e6dd56717fb93d6cb3d5ea07b7ffdc0c0af2f86ab8485c73cd3fef280316fe282d96b4be42fd9df28d562c77edecef9c923fe9f6a069a346c1b7b33e9cc76c3e46dc4bacfc191cd3c8afcbc12e52eeaa7c9127ed6412c70ebee6b52dbc825971322c5eaea9adfb6673a54fddf37696757ff4aafa433f6da3531b23988abba61d3ba7beeecbb40db56935f1e7661d3812798fb95131b69eefe68f25fbf7ee7dd870517a79b4cecf0bb73ac439d5a7b7942c3cdef156ac284f31467ba5e0b39a4d8f569c303bba2c52e1b8f98c0ce91d4a96b33ffcaa985c94b2c06ec781a0c9e9d3bc2670ef1429e09b782fb323d9692607dbe9a30589dbbb6e479efbbe72d62af9f038b605f38ced7d32266f751189ff6a68f2d4b63d94c5f88cf575f7cfbbc3e3fae64b5cdc7d4cadf8ebc24bb2894b657e733d78fb3e6d47dca4bdfc1d264c9d2562dfaff4396cb83cfd94c2dc7766cbd3d218fde61f12e6b9767ed36dc625138d6778f7187a28075597196a6d522f9ac9b8e60a77dc094daf395ec7175c0f63f1326a5f257762b172c517dfbdf6ce7ed7f518129fac14fa77d84140d9e2f92791a34b7e3d7f27a4e82c7c66fbf38589266a16d3a2db4eba4e0d7b646e98fdbdea9af4e3a7739a0acb5c53f65c70c24ca002361a978eee8e5a59adbce3c786730719839d1fce3e894d8c12bdc48a31fd64126c68e6777268e677cedbc9c4a2bf26538a011f60725ecb801f24e097665c40403fe7fefa0f719efb64a6f1b7ca591d5aaa36bfece6cb15dfc37ea65d6cf37fd3b971b6848de6dc1bd7debe378909b2bdd6afc061fd29fa6e59a3935dea85d34213658e093f3a776abee3b523ab2eb933771ee2f0718c8d55ce0fff7e4b4a3395fba9bd8949656292c2a18d5cb97dcfcfccaeba72f6d59b2f824df5f5ca6eff5f1db96e57b14fe370a9b0cca7aeca4e7d4b5b33a9b06496a936455325669e8b489e2c1e5bf5e55666cf0b57070f7585cf35d922eaf6a57f4d583f2e8d8e6cbf31b7f1d3c9d432b377166db5f61bf7695b6ed67cc4f2e58bc4d1a7b39fe79e63f1582adbac7831454fc322c952de71f9d463ff73b86ec5bcd0e5519176645bc29572fa7df1cf49d3df24ea2e10d00b9f1fdd2c3c4b32d0f3e8a6355bf57708142c6ae3e8e0ff97ae2fe0e9f1a09b5b488140f8317dbed5ba6f8acc3e09bb0299aae517394dea2eb96419548530587fbffde1a7c734b7a625d2193a179630bf3634942998f4517fd6c71b0155779c7f7ff9686daf705934ed00d38f9dedfc5a8b58ba2f30b44466e88308831f3b96186d67c845b6e8de5a7488c75550f328040d84141c60faf181bb59e0e45710def1242c523632b128a984814ae088bb4a55457efea747cf9ec61a2a7aaf7f74cc600b012d5c145a49483f37162f2715270f772f6f6ac097342f74698aa7dafab9714c563029fcc0c0a1f6dbc1049769bc0fb66d5e9ec230104933a9b8b86058c7d3ab866681ea0b4b362847edd3ecff7e22df3661dd5a9eb50c6c4e57171c5c67bebef4ec9e87d33bb9773f9e9f701a49a9492dd781dfb5075a6f58cfdb32d3edd0546dbd035167b8c4266d0c083cb22f5479fa8f6eae66c12d293b5a18577c48fd3355d363bdd5ef7cb6acc5fb7630cf3feda55f5678d57b87f786794f055d8eb1c5d23a8c7e08c91cf439e4237bd867c71da69d779876dd61dab794e5e73ef6090bf9272ce46f5fca3161217fcb69c923b7246ecc976407000000ffff01";
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

        let config = crate::config::Config {
            chain: ChainConfig::optimism_sepolia(),
            ..Default::default()
        };
        let batch = SpanBatch::decode(&batch_data[1..], 0, config.chain.l2_chain_id).unwrap();

        assert_eq!(
            batch.transactions.len(),
            batch.block_tx_counts.iter().sum::<u64>() as usize
        );

        assert_eq!(batch.l1_inclusion_block, 0);

        println!("starting epoch: {}", batch.start_epoch_num());

        let inputs = batch.block_inputs(&config);
        inputs.iter().for_each(|input| {
            let block_number = (input.timestamp - config.chain.l2_genesis.timestamp) / 2;
            println!("block: {}, epoch: {}", block_number, input.epoch);
            input.transactions.iter().for_each(|tx| {
                println!("{:?}", H256::from(keccak256(&tx.0)));
            });
        });

        // println!("{:?}", batch.block_inputs(&config))
    }
}
