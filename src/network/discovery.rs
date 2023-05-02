use std::{
    net::{Ipv4Addr, SocketAddr},
    str::FromStr,
    time::Duration,
};

use discv5::{
    enr::{CombinedKey, Enr, EnrBuilder, NodeId},
    Discv5, Discv5Config,
};
use ethers::utils::rlp;
use eyre::Result;
use tokio::{
    sync::mpsc::{self, Receiver},
    time::sleep,
};
use unsigned_varint::{decode, encode};

pub fn start(addr: SocketAddr, chain_id: u64) -> Result<Receiver<Peer>> {
    let bootnodes = bootnodes();
    let mut disc = create_disc()?;

    let (sender, recv) = mpsc::channel::<Peer>(256);

    tokio::spawn(async move {
        bootnodes.into_iter().for_each(|enr| _ = disc.add_enr(enr));
        disc.start(addr).await.unwrap();

        tracing::info!("started peer discovery");

        loop {
            let target = NodeId::random();
            match disc.find_node(target).await {
                Ok(nodes) => {
                    let peers = nodes
                        .iter()
                        .filter(|node| is_valid_node(node, chain_id))
                        .flat_map(Peer::try_from);

                    for peer in peers {
                        _ = sender.send(peer).await;
                    }
                }
                Err(err) => {
                    tracing::warn!("discovery error: {:?}", err);
                }
            }

            sleep(Duration::from_secs(30)).await;
        }
    });

    Ok(recv)
}

fn is_valid_node(node: &Enr<CombinedKey>, chain_id: u64) -> bool {
    node.get_raw_rlp("opstack")
        .map(|opstack| {
            OpStackEnrData::try_from(opstack)
                .map(|opstack| opstack.chain_id == chain_id && opstack.version == 0)
                .unwrap_or_default()
        })
        .unwrap_or_default()
}

fn create_disc() -> Result<Discv5> {
    let opstack = OpStackEnrData {
        chain_id: 420,
        version: 0,
    };
    let opstack_data: Vec<u8> = opstack.into();

    let key = CombinedKey::generate_secp256k1();
    let enr = EnrBuilder::new("v4")
        .add_value_rlp("opstack", opstack_data.into())
        .build(&key)?;
    let config = Discv5Config::default();

    Discv5::new(enr, key, config).map_err(|_| eyre::eyre!("could not create disc service"))
}

#[derive(Debug)]
pub struct Peer {
    pub ip: Ipv4Addr,
    pub port: u16,
}

impl TryFrom<&Enr<CombinedKey>> for Peer {
    type Error = eyre::Report;

    fn try_from(value: &Enr<CombinedKey>) -> Result<Self> {
        let ip = value.ip4().ok_or(eyre::eyre!("missing ip"))?;
        let port = value.tcp4().ok_or(eyre::eyre!("missing port"))?;

        Ok(Self { ip, port })
    }
}

#[derive(Debug)]
struct OpStackEnrData {
    chain_id: u64,
    version: u64,
}

impl TryFrom<&[u8]> for OpStackEnrData {
    type Error = eyre::Report;

    fn try_from(value: &[u8]) -> Result<Self> {
        let bytes: Vec<u8> = rlp::decode(value)?;
        let (chain_id, rest) = decode::u64(&bytes)?;
        let (version, _) = decode::u64(rest)?;

        Ok(Self { chain_id, version })
    }
}

impl From<OpStackEnrData> for Vec<u8> {
    fn from(value: OpStackEnrData) -> Vec<u8> {
        let mut chain_id_buf = encode::u128_buffer();
        let chain_id_slice = encode::u128(value.chain_id as u128, &mut chain_id_buf);

        let mut version_buf = encode::u128_buffer();
        let version_slice = encode::u128(value.version as u128, &mut version_buf);

        let opstack = [chain_id_slice, version_slice].concat();

        rlp::encode(&opstack).to_vec()
    }
}

fn bootnodes() -> Vec<Enr<CombinedKey>> {
    let bootnodes = vec![
        "enr:-J64QBbwPjPLZ6IOOToOLsSjtFUjjzN66qmBZdUexpO32Klrc458Q24kbty2PdRaLacHM5z-cZQr8mjeQu3pik6jPSOGAYYFIqBfgmlkgnY0gmlwhDaRWFWHb3BzdGFja4SzlAUAiXNlY3AyNTZrMaECmeSnJh7zjKrDSPoNMGXoopeDF4hhpj5I0OsQUUt4u8uDdGNwgiQGg3VkcIIkBg",
        "enr:-J64QAlTCDa188Hl1OGv5_2Kj2nWCsvxMVc_rEnLtw7RPFbOfqUOV6khXT_PH6cC603I2ynY31rSQ8sI9gLeJbfFGaWGAYYFIrpdgmlkgnY0gmlwhANWgzCHb3BzdGFja4SzlAUAiXNlY3AyNTZrMaECkySjcg-2v0uWAsFsZZu43qNHppGr2D5F913Qqs5jDCGDdGNwgiQGg3VkcIIkBg",
        "enr:-J24QGEzN4mJgLWNTUNwj7riVJ2ZjRLenOFccl2dbRFxHHOCCZx8SXWzgf-sLzrGs6QgqSFCvGXVgGPBkRkfOWlT1-iGAYe6Cu93gmlkgnY0gmlwhCJBEUSHb3BzdGFja4OkAwCJc2VjcDI1NmsxoQLuYIwaYOHg3CUQhCkS-RsSHmUd1b_x93-9yQ5ItS6udIN0Y3CCIyuDdWRwgiMr",
    ];

    bootnodes
        .iter()
        .filter_map(|enr| Enr::from_str(enr).ok())
        .collect()
}
