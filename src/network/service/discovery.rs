//! Module handles Discv5 discovery

use std::{str::FromStr, time::Duration};

use super::enr::OpStackEnrData;
use discv5::{
    enr::{CombinedKey, Enr, EnrBuilder, NodeId},
    Discv5, Discv5Config,
};
use anyhow::Result;
use tokio::{
    sync::mpsc::{self, Receiver},
    time::sleep,
};

use super::types::{NetworkAddress, Peer};

/// Starts the [Discv5] discovery service and continually tries to find new peers.
/// Returns a [Receiver] to receive [Peer] structs
pub fn start(addr: NetworkAddress, chain_id: u64) -> Result<Receiver<Peer>> {
    let bootnodes = bootnodes();
    let mut disc = create_disc(chain_id)?;

    let (sender, recv) = mpsc::channel::<Peer>(256);

    tokio::spawn(async move {
        bootnodes.into_iter().for_each(|enr| _ = disc.add_enr(enr));
        disc.start(addr.into()).await.unwrap();

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

            sleep(Duration::from_secs(10)).await;
        }
    });

    Ok(recv)
}

/// Returns `true` if a node [Enr] contains an `opstack` key and is on the same network.
fn is_valid_node(node: &Enr<CombinedKey>, chain_id: u64) -> bool {
    node.get_raw_rlp("opstack")
        .map(|opstack| {
            OpStackEnrData::try_from(opstack)
                .map(|opstack| opstack.chain_id == chain_id && opstack.version == 0)
                .unwrap_or_default()
        })
        .unwrap_or_default()
}

/// Generates an [Enr] and creates a [Discv5] service struct
fn create_disc(chain_id: u64) -> Result<Discv5> {
    let opstack = OpStackEnrData {
        chain_id,
        version: 0,
    };
    let opstack_data: Vec<u8> = opstack.into();

    let key = CombinedKey::generate_secp256k1();
    let enr = EnrBuilder::new("v4")
        .add_value_rlp("opstack", opstack_data.into())
        .build(&key)?;
    let config = Discv5Config::default();

    Discv5::new(enr, key, config).map_err(|_| anyhow::anyhow!("could not create disc service"))
}

/// Default bootnodes to use. Currently consists of 2 Base bootnodes & 1 Optimism bootnode.
fn bootnodes() -> Vec<Enr<CombinedKey>> {
    let bootnodes = [
        "enr:-J64QBbwPjPLZ6IOOToOLsSjtFUjjzN66qmBZdUexpO32Klrc458Q24kbty2PdRaLacHM5z-cZQr8mjeQu3pik6jPSOGAYYFIqBfgmlkgnY0gmlwhDaRWFWHb3BzdGFja4SzlAUAiXNlY3AyNTZrMaECmeSnJh7zjKrDSPoNMGXoopeDF4hhpj5I0OsQUUt4u8uDdGNwgiQGg3VkcIIkBg",
        "enr:-J64QAlTCDa188Hl1OGv5_2Kj2nWCsvxMVc_rEnLtw7RPFbOfqUOV6khXT_PH6cC603I2ynY31rSQ8sI9gLeJbfFGaWGAYYFIrpdgmlkgnY0gmlwhANWgzCHb3BzdGFja4SzlAUAiXNlY3AyNTZrMaECkySjcg-2v0uWAsFsZZu43qNHppGr2D5F913Qqs5jDCGDdGNwgiQGg3VkcIIkBg",
        "enr:-J24QGEzN4mJgLWNTUNwj7riVJ2ZjRLenOFccl2dbRFxHHOCCZx8SXWzgf-sLzrGs6QgqSFCvGXVgGPBkRkfOWlT1-iGAYe6Cu93gmlkgnY0gmlwhCJBEUSHb3BzdGFja4OkAwCJc2VjcDI1NmsxoQLuYIwaYOHg3CUQhCkS-RsSHmUd1b_x93-9yQ5ItS6udIN0Y3CCIyuDdWRwgiMr",

        // Base bootnodes
        "enr:-J24QNz9lbrKbN4iSmmjtnr7SjUMk4zB7f1krHZcTZx-JRKZd0kA2gjufUROD6T3sOWDVDnFJRvqBBo62zuF-hYCohOGAYiOoEyEgmlkgnY0gmlwhAPniryHb3BzdGFja4OFQgCJc2VjcDI1NmsxoQKNVFlCxh_B-716tTs-h1vMzZkSs1FTu_OYTNjgufplG4N0Y3CCJAaDdWRwgiQG",
        "enr:-J24QH-f1wt99sfpHy4c0QJM-NfmsIfmlLAMMcgZCUEgKG_BBYFc6FwYgaMJMQN5dsRBJApIok0jFn-9CS842lGpLmqGAYiOoDRAgmlkgnY0gmlwhLhIgb2Hb3BzdGFja4OFQgCJc2VjcDI1NmsxoQJ9FTIv8B9myn1MWaC_2lJ-sMoeCDkusCsk4BYHjjCq04N0Y3CCJAaDdWRwgiQG",
        "enr:-J24QDXyyxvQYsd0yfsN0cRr1lZ1N11zGTplMNlW4xNEc7LkPXh0NAJ9iSOVdRO95GPYAIc6xmyoCCG6_0JxdL3a0zaGAYiOoAjFgmlkgnY0gmlwhAPckbGHb3BzdGFja4OFQgCJc2VjcDI1NmsxoQJwoS7tzwxqXSyFL7g0JM-KWVbgvjfB8JA__T7yY_cYboN0Y3CCJAaDdWRwgiQG",
        "enr:-J24QHmGyBwUZXIcsGYMaUqGGSl4CFdx9Tozu-vQCn5bHIQbR7On7dZbU61vYvfrJr30t0iahSqhc64J46MnUO2JvQaGAYiOoCKKgmlkgnY0gmlwhAPnCzSHb3BzdGFja4OFQgCJc2VjcDI1NmsxoQINc4fSijfbNIiGhcgvwjsjxVFJHUstK9L1T8OTKUjgloN0Y3CCJAaDdWRwgiQG",
        "enr:-J24QG3ypT4xSu0gjb5PABCmVxZqBjVw9ca7pvsI8jl4KATYAnxBmfkaIuEqy9sKvDHKuNCsy57WwK9wTt2aQgcaDDyGAYiOoGAXgmlkgnY0gmlwhDbGmZaHb3BzdGFja4OFQgCJc2VjcDI1NmsxoQIeAK_--tcLEiu7HvoUlbV52MspE0uCocsx1f_rYvRenIN0Y3CCJAaDdWRwgiQG",
    ];

    bootnodes
        .iter()
        .filter_map(|enr| Enr::from_str(enr).ok())
        .collect()
}
