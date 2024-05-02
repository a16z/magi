//! Network Module
//!
//! Contains [handlers] and [service] peer to peer networking components.

pub mod handlers;
pub use handlers::{BlockHandler, Handler};

pub mod service;
pub use service::Service;
