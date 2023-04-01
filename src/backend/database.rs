use eyre::Result;
use std::path::Path;
use uuid::Uuid;

use super::types::*;

/// Memory backend to store blocks and transactions.
#[derive(Debug, Clone)]
pub struct Database {
    /// Internal [seld](sled) db
    db: sled::Db,
}

impl Default for Database {
    fn default() -> Self {
        let loc = Self::fallback_location();
        Self {
            db: sled::open(loc).unwrap(),
        }
    }
}

impl Database {
    /// Creates a new database.
    pub fn new<P: AsRef<Path>>(path: P, network: &str) -> Self {
        Self {
            db: Self::try_construct_db(path, network),
        }
    }

    /// Gets a random location to use as a fallback
    pub fn fallback_location() -> String {
        format!("/tmp/magi/{}", Uuid::new_v4())
    }

    /// Clear wipes a database sled location.
    ///
    /// ## Warning
    ///
    /// Be careful when using this function, as it will delete all data.
    pub fn clear(&self) -> Result<()> {
        self.db.clear().map_err(|e| eyre::eyre!(e))
    }

    /// ## [`flush_async`]
    ///
    /// Flushes the database to disk asynchronously.
    ///
    /// Internally, this function uses [`sled::Db::flush_async`] which
    /// asynchronously flushes all dirty IO buffers and calls fsync.
    /// If this succeeds, it is guaranteed that all previous writes will
    /// be recovered if the system crashes.
    /// Returns the number of bytes flushed during this call.
    ///
    /// Flushing can take a long time.
    pub async fn flush_async(&self) -> Result<usize> {
        self.db.flush_async().await.map_err(|e| eyre::eyre!(e))
    }

    /// Attempts to construct a database for a given location. The path will have the network
    /// appended to it to prevent conflicts between magi running on different networks.
    ///
    /// ## Panics
    ///
    /// This function will panic if neither the given file location
    /// nor a temporary location can be used to construct a database.
    fn try_construct_db<P: AsRef<Path>>(path: P, network: &str) -> sled::Db {
        match sled::open(path.as_ref().join(network)) {
            Ok(db) => db,
            Err(e) => {
                tracing::error!("Failed to open database: {}", e);
                let new_loc = Self::fallback_location();
                tracing::debug!("Optimistically creating new database at {}", new_loc);
                sled::open(new_loc).unwrap()
            }
        }
    }

    /// Returns the internal sled database.
    pub fn inner(&self) -> &sled::Db {
        &self.db
    }

    /// Inserts a [`HeadInfo`] into the database with the key `HEAD_INFO`.
    pub fn write_head(&self, head: HeadInfo) -> Result<()> {
        self.db.insert("HEAD_INFO", head)?;
        Ok(())
    }

    /// Reads the most recent [`HeadInfo`].
    ///
    /// ## Warning
    ///
    /// This function will return [`None`] if the database is empty or panics.
    pub fn read_head(&self) -> Option<HeadInfo> {
        let head = self.db.get("HEAD_INFO").unwrap_or_default();
        head.map(HeadInfo::try_from).transpose().unwrap_or_default()
    }
}
