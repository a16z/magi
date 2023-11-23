#![allow(unused_imports)]
use std::{net::Ipv4Addr, str::FromStr};

use discv5::{
    enr::{CombinedKey, EnrBuilder},
    Enr,
};
use ethers::types::Address;
use eyre::Result;

use ethers::utils::rlp;
use libp2p::gossipsub::IdentTopic;
use magi::{
    network::{handlers::block_handler::BlockHandler, handlers::Handler, service::Service},
    telemetry,
};
use tokio::sync::watch;
use unsigned_varint::encode;

#[derive(Debug)]
pub struct OpStackEnrData {
    chain_id: u64,
    version: u64,
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

#[tokio::main]
async fn main() -> Result<()> {
    let _guards = telemetry::init(false, None, None);

    let addr = "0.0.0.0:9221".parse()?;
    let chain_id = 901;
    let (_, recv) = watch::channel(Address::from_str(
        "0xF64c29538cAE4E69eac62c50CDfebAC22b378044",
    )?);
    let (block_handler, block_recv) = BlockHandler::new(chain_id, recv);
    // channel for sending new blocks to peers
    let (_sender, receiver) = tokio::sync::mpsc::channel(1_000);

    // For generation of new Enr uncomment the following code and add private key.
    // Generate private key.
    // let mut pk =
    //     hex::decode("private key")?;
    // let private_key = CombinedKey::secp256k1_from_bytes(&mut pk)?;

    // // Get RLP for optimism.
    // let opstack = OpStackEnrData {
    //     chain_id,
    //     version: 0,
    // };
    // let opstack_data: Vec<u8> = opstack.into();

    // // Get ERN.
    // let enr = EnrBuilder::new("v4")
    //         .add_value_rlp("opstack", opstack_data.into())
    //         .ip4(Ipv4Addr::new(127, 0, 0, 1))
    //         .tcp4(9980)
    //         .udp4(9980)
    //         .build(&private_key)?;
    // println!("ENR: {:?}", enr);
    // let bootnodes = vec![enr];

    let bootnodes: Vec<discv5::enr::Enr<CombinedKey>> =
        vec![Enr::from_str("enr:-Je4QKqISnjZwcUSRQqLTbOoqFvmQX8sVlPIud5sWPrUp_8hPJXnzSyY-fqXhzqWGKDHjNSLJRbBGjC9VILm_HGuhHkBgmlkgnY0gmlwhH8AAAGHb3BzdGFja4OFBwCJc2VjcDI1NmsxoQMqv564GlblO4zWKiGSn0-lcr70dYrzwiieFETLNEy8xoN0Y3CCJvyDdWRwgib8").map_err(|e| eyre::eyre!("err: {}", e))?];

    Service::new(
        addr,
        chain_id,
        Some(bootnodes),
        None,
        None,
        IdentTopic::new(block_handler.topic().to_string()),
    )
    .add_handler(Box::new(block_handler))
    .start(receiver)?;

    while let Ok(payload) = block_recv.recv() {
        tracing::info!("received unsafe block with hash: {:?}", payload.block_hash);
    }

    Ok(())
}
