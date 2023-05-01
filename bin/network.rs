use std::{net::SocketAddr, str::FromStr, time::Duration};

use discv5::{enr::{CombinedKey, EnrBuilder, NodeId}, Discv5, Discv5Config, Enr};
use ethers::utils;
use unsigned_varint::{encode, decode};
use eyre::Result;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<()> {
    let addr = "0.0.0.0:9000".parse::<SocketAddr>()?;

    let mut chain_id_buf = encode::u128_buffer();
    let chain_id_slice = encode::u128(420, &mut chain_id_buf);

    let mut version_buf = encode::u128_buffer();
    let version_slice = encode::u128(0, &mut version_buf);

    let opstack = [chain_id_slice, version_slice].concat();
    println!("{:?}", opstack);
    let opstack_data = utils::rlp::encode(&opstack);

    
    let key = CombinedKey::generate_secp256k1();
    let enr = EnrBuilder::new("v4").add_value_rlp("opstack", opstack_data.into()).build(&key)?;
    let config = Discv5Config::default();

    let mut discv5: Discv5 = Discv5::new(enr, key, config).map_err(convert_err)?;
    discv5.start(addr).await.map_err(convert_err)?;

    let enr = Enr::from_str("enr:-J64QBbwPjPLZ6IOOToOLsSjtFUjjzN66qmBZdUexpO32Klrc458Q24kbty2PdRaLacHM5z-cZQr8mjeQu3pik6jPSOGAYYFIqBfgmlkgnY0gmlwhDaRWFWHb3BzdGFja4SzlAUAiXNlY3AyNTZrMaECmeSnJh7zjKrDSPoNMGXoopeDF4hhpj5I0OsQUUt4u8uDdGNwgiQGg3VkcIIkBg").unwrap();

    discv5.add_enr(enr).unwrap();

    println!("listening");

    loop {
        println!("searching for new peers");

        let target = NodeId::random();
        match discv5.find_node(target).await {
            Ok(nodes) => {
                for node in &nodes {
                    if node.get_raw_rlp("opstack").is_some() {
                        let opstack_enr_data_raw: Vec<u8> = utils::rlp::decode(node.get_raw_rlp("opstack").unwrap()).unwrap();
                        let opstack_enr_data = OpStackEnrData::try_from(opstack_enr_data_raw.as_slice()).unwrap();
                        println!("{:?}", opstack_enr_data);
                    }
                }
                println!("found {} peers", nodes.len());
            },
            Err(err) => {
                println!("{:?}", err);
            },
        }

        println!("peers: {}", discv5.connected_peers());
        sleep(Duration::from_secs(1)).await;
    }
}

fn convert_err<E>(_err: E) -> eyre::Report {
    eyre::eyre!("error")
}

#[derive(Debug)]
struct OpStackEnrData {
    chain_id: u64,
    version: u64,
}

impl TryFrom<&[u8]> for OpStackEnrData {
    type Error = eyre::Report;

    fn try_from(value: &[u8]) -> Result<Self> {
        let (chain_id, rest) = decode::u64(&value)?;
        let (version, _) = decode::u64(rest)?;

        Ok(Self { chain_id, version })
     } 
}
