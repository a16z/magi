// use reth_discv4::bootnodes::mainnet_nodes;
// use reth_network::config::rng_secret_key;
// use reth_network::{NetworkConfig, NetworkManager};
// use reth_provider::test_utils::NoopProvider;
// use reth_transaction_pool::TransactionPool;

// async fn launch<Pool: TransactionPool>(pool: Pool) {
//     // This block provider implementation is used for testing purposes.
//     let client = NoopProvider::default();

//     // The key that's used for encrypting sessions and to identify our node.
//     let local_key = rng_secret_key();

//     let config =
//         NetworkConfig::<NoopProvider>::builder(local_key).boot_nodes(mainnet_nodes()).build(client.clone());

//     // create the network instance
//     let (handle, network, transactions, request_handler) = NetworkManager::builder(config)
//         .await
//         .unwrap()
//         .transactions(pool)
//         .request_handler(client)
//         .split_with_handle();
// }


use discv5::{
    enr,
    enr::{k256, CombinedKey},
    Discv5, Discv5ConfigBuilder, Discv5Event,
};
use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};
use tracing::{info, warn};




/// Peer Manager Configuration
#[derive(Debug, Clone)]
pub struct PeerConfig {}

/// P2P Manager
pub struct PeerManager {
    /// Peer configuration
    config: PeerConfig,
}

impl PeerManager {
    /// Create a new peer Manager
    pub fn new() -> Self {
        let config = PeerConfig {};
        Self { config }
    }
}
