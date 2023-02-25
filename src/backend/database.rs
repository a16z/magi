use eyre::Result;
use std::collections::HashMap;
use uuid::Uuid;

use super::types::*;

/// Memory backend to store blocks and transactions.
#[derive(Debug, Clone)]
pub struct Database {
    /// A map of block hash to the block object.
    blocks: HashMap<BlockHash, BlockNumber>,
    /// A map from block number to block hash.
    hashes: HashMap<BlockNumber, BlockHash>,
    /// Internal [seld](sled) db
    db: sled::Db,
}

impl Database {
    /// Creates a new database.
    pub fn new(loc: &str) -> Self {
        Self {
            blocks: HashMap::new(),
            hashes: HashMap::new(),
            db: Self::try_construct_db(loc),
        }
    }

    /// Clear wipes a database sled location.
    ///
    /// ## Warning
    ///
    /// Be careful when using this function, as it will delete all data.
    pub fn clear(&self) -> Result<()> {
        self.db.clear().map_err(|e| eyre::eyre!(e))
    }

    /// Attempts to construct a database for a given location.
    ///
    /// ## Panics
    ///
    /// This function will panic if neither the given file location
    /// nor a temporary location can be used to construct a database.
    fn try_construct_db(loc: &str) -> sled::Db {
        match sled::open(loc) {
            Ok(db) => db,
            Err(e) => {
                tracing::error!("Failed to open database: {}", e);
                let new_loc = format!("/tmp/magi/{}", Uuid::new_v4());
                tracing::debug!("Optimistically creating new database at {}", new_loc);
                sled::open(new_loc).unwrap()
            }
        }
    }

    /// Returns the internal sled database.
    pub fn inner(&self) -> &sled::Db {
        &self.db
    }

    /// Returns the block number for a given block hash.
    pub fn block_number(&self, hash: &BlockHash) -> Option<BlockNumber> {
        self.blocks.get(hash).copied()
    }

    /// Returns the block hash for a given block number.
    pub fn block_hash(&self, number: &BlockNumber) -> Option<BlockHash> {
        self.hashes.get(number).copied()
    }

    /// Internal function to write a block to the database.
    pub fn write_block(&mut self, block: ConstructedBlock) -> Result<()> {
        let number = block.number;
        if let Some(hash) = block.hash {
            self.hashes.insert(number, hash);
            self.blocks.insert(hash, number);
        }
        let dbid = DatabaseId::from(&block);
        let ivec: sled::IVec = block.into();
        let qid = dbid.as_bytes();
        self.db.insert(qid, ivec)?;
        Ok(())
    }

    /// Reads a block from cache, or the database.
    pub fn read_block(&mut self, id: impl Into<DatabaseId>) -> Result<ConstructedBlock> {
        let dbid = id.into();
        let qid = dbid.as_bytes();
        let block = self.db.get(qid)?;
        block.map(Into::into).ok_or(eyre::eyre!("Block not found"))
    }
}
