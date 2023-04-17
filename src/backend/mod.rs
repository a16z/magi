#![warn(missing_debug_implementations)]
#![deny(rustdoc::broken_intra_doc_links)]

//! # Minimal Block Database
//!
//! This exposes a minimal block database that can be used to store and retrieve
//! [HeadInfo](crate::types::HeadInfo) to persistent storage.
//!
//! ## Example
//!
//! ```rust
//! use magi::backend::{Database, HeadInfo};
//!
//! // Note: this will panic if both `/tmp/magi` and the hardcoded temporary location cannot be used.
//! let mut db = Database::new("/tmp/magi", "optimism-goerli");
//! let head = HeadInfo::default();
//! db.write_head(head.clone()).unwrap();
//! let read_head = db.read_head().unwrap();
//! assert_eq!(head, read_head);
//! db.clear().unwrap();
//! ```

/// Core Backend Types
mod types;
pub use types::HeadInfo;

/// Core Backend Database
mod database;
pub use database::Database;

#[cfg(test)]
mod tests {
    use super::database::Database;
    use super::types::HeadInfo;

    #[test]
    fn test_backend_db() {
        let db = Database::new("/tmp/magi", "optimism-goerli");
        let head = HeadInfo::default();
        db.write_head(head.clone()).unwrap();
        let read_head = db.read_head().unwrap();
        assert_eq!(head, read_head);
        db.clear().unwrap();
    }
}
