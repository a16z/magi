use magi::net::bootnodes;

// #[cfg(feature = "peers")]
// use discv5::{enr, enr::CombinedKey, Discv5, Discv5Config};

#[cfg(feature = "peers")]
#[test]
fn test_bootnodes() {
    let nodes = bootnodes::optimism_mainnet_nodes();
    println!("Nodes: {:?}", nodes);
    assert_eq!(nodes.len(), 3);
}

// #[cfg(feature = "peers")]
// #[tokio::test]
// async fn connect_bootnodes() {
//     let mut discv5: Discv5 = Discv5::new(enr, enr_key, config).unwrap();
//     discv5.start(listen_addr).await.unwrap();

// }

#[cfg(feature = "peers")]
#[tokio::test]
async fn test_get_p2p_blocks() {
    // TODO:
}
