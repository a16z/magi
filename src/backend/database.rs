use eyre::Result;
use std::{collections::HashMap, path::Path};
use uuid::Uuid;

use super::types::*;

/// Memory backend to store blocks and transactions.
#[derive(Debug, Clone)]
pub struct Database {
    /// A map of block hash to the block object.
    blocks: HashMap<BlockHash, BlockNumber>,
    /// A map from block number to block hash.
    hashes: HashMap<BlockNumber, BlockHash>,
    /// A map from transaction hash to the block (number) that contains it.
    transactions: HashMap<TxHash, BlockNumber>,
    /// Timestamp Mapping
    timestamps: HashMap<Timestamp, Vec<BlockNumber>>,
    /// L1 Origin Block Hash
    l1_origin_block_hash: HashMap<BlockHash, Vec<BlockNumber>>,
    /// L1 Origin Block Number
    l1_origin_block_number: HashMap<BlockNumber, Vec<BlockNumber>>,
    /// Internal [seld](sled) db
    db: sled::Db,
}

impl Default for Database {
    fn default() -> Self {
        let loc = Self::fallback_location();
        Self {
            blocks: Default::default(),
            hashes: Default::default(),
            timestamps: Default::default(),
            transactions: Default::default(),
            l1_origin_block_hash: Default::default(),
            l1_origin_block_number: Default::default(),
            db: sled::open(loc).unwrap(),
        }
    }
}

impl Database {
    /// Creates a new database.
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            db: Self::try_construct_db(path),
            ..Default::default()
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

    /// Attempts to construct a database for a given location.
    ///
    /// ## Panics
    ///
    /// This function will panic if neither the given file location
    /// nor a temporary location can be used to construct a database.
    fn try_construct_db<P: AsRef<Path>>(path: P) -> sled::Db {
        match sled::open(path) {
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

    /// Returns the block number for a given block hash.
    pub fn block_number(&self, hash: &BlockHash) -> Option<BlockNumber> {
        self.blocks.get(hash).copied()
    }

    /// Returns the block hash for a given block number.
    pub fn block_hash(&self, number: &BlockNumber) -> Option<BlockHash> {
        self.hashes.get(number).copied()
    }

    /// Inserts a [`HeadInfo`] into the database with the key `HEAD_INFO`.
    pub fn write_head(&self, head: HeadInfo) -> Result<()> {
        self.db.insert("HEAD_INFO", head)?;
        Ok(())
    }

    /// Internal function to write a block to the database.
    pub fn write_block(&mut self, block: ConstructedBlock) -> Result<()> {
        let number = block.number;
        if let Some(hash) = block.hash {
            self.hashes.insert(number, hash);
            self.blocks.insert(hash, number);
        }
        match self.timestamps.get(&block.timestamp) {
            Some(v) => {
                let mut v = v.clone();
                v.push(number);
                self.timestamps.insert(block.timestamp, v);
            }
            None => {
                self.timestamps.insert(block.timestamp, vec![number]);
            }
        }
        if let Some(l1_origin_block_hash) = block.l1_origin_block_hash {
            match self.l1_origin_block_hash.get(&l1_origin_block_hash) {
                Some(v) => {
                    let mut v = v.clone();
                    v.push(number);
                    self.l1_origin_block_hash.insert(l1_origin_block_hash, v);
                }
                None => {
                    self.l1_origin_block_hash
                        .insert(l1_origin_block_hash, vec![number]);
                }
            }
        }
        if let Some(l1_origin_block_number) = block.l1_origin_block_number {
            match self.l1_origin_block_number.get(&l1_origin_block_number) {
                Some(v) => {
                    let mut v = v.clone();
                    v.push(number);
                    self.l1_origin_block_number
                        .insert(l1_origin_block_number, v);
                }
                None => {
                    self.l1_origin_block_number
                        .insert(l1_origin_block_number, vec![number]);
                }
            }
        }
        for tx in &block.transactions {
            self.transactions.insert(tx.hash, number);
        }
        let dbid = DatabaseId::from(&block);
        let ivec: sled::IVec = block.into();
        let qid = dbid.as_bytes();
        self.db.insert(qid, ivec)?;
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

    /// Reads a block from cache, or the database.
    pub fn read_block(&self, id: impl Into<DatabaseId>) -> Result<ConstructedBlock> {
        let dbid = id.into();
        let qid = dbid.as_bytes();
        let block = self.db.get(qid)?;
        block.map(Into::into).ok_or(eyre::eyre!("Block not found"))
    }

    /// Fetches a block with a given transaction hash.
    pub fn block_by_tx_hash(&self, hash: TxHash) -> Option<ConstructedBlock> {
        let block_number = self.transactions.get(&hash).copied()?;
        self.read_block(block_number).ok()
    }

    /// Fetches blocks with a given timestamp.
    pub fn blocks_by_timestamp(&self, timestamp: Timestamp) -> Vec<ConstructedBlock> {
        let block_numbers = self.timestamps.get(&timestamp).cloned().unwrap_or_default();

        block_numbers
            .iter()
            .filter_map(|&n| self.read_block(n).ok())
            .collect()
    }

    /// Fetches blocks with a given L1 Origin Block Hash.
    pub fn blocks_by_origin_hash(&self, hash: BlockHash) -> Vec<ConstructedBlock> {
        let block_numbers = self
            .l1_origin_block_hash
            .get(&hash)
            .cloned()
            .unwrap_or_default();

        block_numbers
            .iter()
            .filter_map(|&n| self.read_block(n).ok())
            .collect()
    }

    /// Fetches blocks with a given L1 Origin Block Number.
    pub fn blocks_by_origin_number(&self, number: BlockNumber) -> Vec<ConstructedBlock> {
        let block_numbers = self
            .l1_origin_block_number
            .get(&number)
            .cloned()
            .unwrap_or_default();

        block_numbers
            .iter()
            .filter_map(|&n| self.read_block(n).ok())
            .collect()
    }
}
