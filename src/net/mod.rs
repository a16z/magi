//! P2P Networking
//!
//! This module contains the networking code for the p2p network.
//! It is responsible for the low-level communication with peers.

/// Bootnodes
pub mod bootnodes;

/// Discus Disc Wrapper Module
mod discus;
pub use discus::*;

/// Network Manager
pub mod manager;

/// Key Utilities
pub mod keys;

/// Peer Data Structures
pub mod peers;

/// Node Record Info
pub mod node_record;

/// P2P Statistics
pub mod stats;

/// Re-export Network Types
pub mod types {
    pub use super::node_record::NodeRecord;
    pub use super::peers::PeerId;
}
