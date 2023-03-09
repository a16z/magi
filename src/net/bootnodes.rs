use crate::net::types::NodeRecord;

/// Default bootnodes
///
/// These are the default bootnodes for the Optimism network.
/// See: https://github.com/ethereum-optimism/optimism/blob/develop/op-node/p2p/config.go#L26-L30
pub const OPTIMISM_MAINNET_BOOTNODES: &[&str] = &[
    "enode://869d07b5932f17e8490990f75a3f94195e9504ddb6b85f7189e5a9c0a8fff8b00aecf6f3ac450ecba6cdabdb5858788a94bde2b613e0f2d82e9b395355f76d1a@34.65.67.101:0?discport=30305",
    "enode://2d4e7e9d48f4dd4efe9342706dd1b0024681bd4c3300d021f86fc75eab7865d4e0cbec6fbc883f011cfd6a57423e7e2f6e104baad2b744c3cafaec6bc7dc92c1@34.65.43.171:0?discport=30305",
    "enode://9d7a3efefe442351217e73b3a593bcb8efffb55b4807699972145324eab5e6b382152f8d24f6301baebbfb5ecd4127bd3faab2842c04cd432bdf50ba092f6645@34.65.109.126:0?discport=30305",
];

/// Returns a list of Optimism Mainnet Bootnodes
pub fn optimism_mainnet_nodes() -> Vec<NodeRecord> {
    parse_nodes(OPTIMISM_MAINNET_BOOTNODES)
}

/// Parses all the nodes
pub fn parse_nodes(nodes: impl IntoIterator<Item = impl AsRef<str>>) -> Vec<NodeRecord> {
    nodes
        .into_iter()
        .map(|s| s.as_ref().parse().unwrap())
        .collect()
}
