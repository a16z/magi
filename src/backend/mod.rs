#![warn(missing_debug_implementations)]
#![deny(rustdoc::broken_intra_doc_links)]

//! # Minimal Block Database
//!
//! This exposes a minimal block database that can be used to store and retrieve
//! [ConstructedBlock](crate::types::ConstructedBlock) to persistent storage.
//!
//! ## Example
//!
//! ```rust
//! use magi::backend::prelude::*;
//! use std::{str::FromStr, path::PathBuf};
//!
//! // Note: this will panic if both `/tmp/magi` and the hardcoded temporary location cannot be used.
//! let mut db = Database::new(&PathBuf::from_str("/tmp/magi").unwrap());
//! let mut block = ConstructedBlock::default();
//! block.hash = Some(BlockHash::from_low_u64_be(1));
//! db.write_block(block.clone()).unwrap();
//! let read_block = db.read_block(block.hash.unwrap()).unwrap();
//! assert_eq!(block, read_block);
//! db.clear().unwrap();
//! ```

/// Core Backend Types
mod types;
pub use types::{BlockHash, BlockNumber, ConstructedBlock, HeadInfo};

/// Core Backend Database
mod database;
pub use database::Database;

pub mod prelude {
    pub use super::types::{BlockHash, BlockNumber, ConstructedBlock};
    pub use super::Database;
}
