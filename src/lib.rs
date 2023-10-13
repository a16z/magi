/// A module for ingesting L1 chain data
pub mod l1;

/// Common types and functions
pub mod common;

/// Configuration management
pub mod config;

/// The derivation pipeline module for deriving the canonical L2 chain
pub mod derive;

/// A module for driving op-geth via the L2 Engine API
pub mod driver;

/// A module for the L2 Engine API
pub mod engine;

/// Peer to peer networking
pub mod network;

/// Application telemetry and logging
pub mod telemetry;

/// RPC module to host rpc server
pub mod rpc;

/// A module to handle running Magi in different sync modes
pub mod runner;

/// A module for the specular features
pub mod specular;
