//! # Magi
//!
//! `Magi` is a Rust implementation of an OP stack rollup node, designed to serve as a replacement for `op-node`. It facilitates interaction with both the L1 (Layer 1) chain and the canonical L2 (Layer 2) chain, enabling efficient data ingestion, processing, and serving via an RPC interface.
//!
//! This crate is structured to provide functionality for running an OP stack rollup node, including configuration management, data derivation, and P2P network communication.
//!
//! ## Features
//!
//! - **L1 Chain Ingestion**: Processes and ingests data from the L1 chain to keep the rollup node synchronized.
//! - **Canonical L2 Chain Derivation**: Derives the canonical L2 chain state based on ingested L1 data.
//! - **L2 Engine API**: Interfaces with `op-geth` for L2 state execution and consensus.
//! - **Networking**: Manages peer-to-peer networking for P2P data dissemination and retrieval.
//! - **RPC Server**: Hosts an RPC server for querying rollup node data.
//! - **Configurable Sync Modes**: Supports different synchronization modes.
//! - **Telemetry and Logging**: Provides application telemetry and logging for monitoring and debugging.
//!
//! ## Modules
//!
//! - [`l1`]: Ingests and processes L1 chain data.
//! - [`common`]: Contains common types and functions used throughout the crate.
//! - [`config`]: Manages configuration settings for the node.
//! - [`mod@derive`]: Handles the derivation pipeline for the L2 chain.
//! - [`driver`]: Drives `op-geth` via the L2 Engine API.
//! - [`engine`]: Provides an implementation of the L2 Engine API.
//! - [`network`]: Manages peer-to-peer networking.
//! - [`telemetry`]: Handles application telemetry and logging.
//! - [`rpc`]: Implements the RPC server for external queries.
//! - [`runner`]: Manages the node's operation in various synchronization modes.
//! - [`version`]: Provides version information for the `magi` crate.
//!
//! ## Getting Started
//!
//! To start using `magi`, add it as a dependency in your `Cargo.toml`:
//!
//! ```toml
//! [dependencies]
//! magi = "0.1.0"
//! ```
//!
//! Then, refer to the individual modules for specific functionality.
//!
#![warn(missing_docs)]
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

/// A module to get current Magi version.
pub mod version;
