#![allow(dead_code)]

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

/// Peer Manager Configuration
#[derive(Debug, Clone)]
pub struct PeerConfig {}

/// P2P Manager
pub struct PeerManager {
    /// Peer configuration
    config: PeerConfig,
}

impl PeerManager {
    // /// Create a new peer Manager
    // pub fn new() -> Self {
    //     let config = PeerConfig {};
    //     Self { config }
    // }
}
