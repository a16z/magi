<img align="right" width="150" height="150" top="100" src="./assets/magi.png">

# magi • [![tests](https://github.com/a16z/magi/actions/workflows/test.yml/badge.svg?label=tests)](https://github.com/a16z/magi/actions/workflows/test.yml) ![license](https://img.shields.io/github/license/a16z/magi?label=license)

`magi` (pronounced may-jai) is an Optimism full node implemented in pure Rust.


### Feature Set

- [ ] Base Chain Watcher
    - [x] Transaction Watcher
    - [x] Block Watcher
    - [x] Deposit Watcher
    - [ ] System Config Watcher
- [ ] Derivation Pipeline
    - [x] Batcher Transaction
    - [x] Channels
    - [ ] Batches
        - [x] Basic Setup
        - [ ] Pruning
    - [x] Paylaod Attributes
        - [x] Basic Setup
        - [x] Attributes Deposited Transactions
            - [x] Basic Setup
            - [x] Handle Sequence Numbers
        - [x] User Deposited Transactions
- [ ] Geth Driver
    - [ ] Engine API Bindings
        - [x] Engine API Trait
        - [ ] Engine API Client Implementation
    - [ ] Driver Loop
- [ ] Backend DB
    - [ ] Modify `ConstructedBlock` type to match output of the Geth Driver
    - [x] Read Blocks By Hash _or_ Number
    - [x] Write Blocks
    - [x] Fetch Block With Transaction
    - [x] Fetch Block By L1 Origin Hash
    - [x] Fetch Block By L1 Origin Number
    - [x] Fetch Block By Timestamp

### Usage

_Prerequisites: Install rust and cargo with `curl https://sh.rustup.rs -sSf | sh`_

**Executable**

Run the main binary with `cargo run --release`

**Tests**

Tests are named wrt their modules, located inside the [tests](./tests) directory.

To run all tests, simply run `cargo test --all`.

### Specification

#### Base Chain Watcher

The base chain watcher is responsible for watching the base chain for new blocks and transactions.

It ...

#### Derivation Pipeline

The derivation pipeline is responsible for deriving the canonical L2 chain from the base chain.

It ...

#### Geth Driver

The geth driver is responsible for serving the Engine API to `magi`.

#### Backend DB

The backend DB is an embedded database that uses [sled](https://docs.rs/sled/latest/sled/index.html) as its backend.
It stores [serde_json](https://docs.rs/serde_json/latest/serde_json/index.html) serialized blocks on disk and provides an interface for querying them. See an example below.

```rust
use magi::backend::prelude::*;

// Note: this will panic if both `/tmp/magi` and the hardcoded temporary location cannot be used.
let mut db = Database::new("/tmp/magi");
let block = ConstructedBlock {
    hash: Some(BlockHash::from([1; 32])),
    ..Default::default()
};
db.write_block(block.clone()).unwrap();
let read_block = db.read_block(block.hash.unwrap()).unwrap();
assert_eq!(block, read_block);
db.clear().unwrap();
```

Notice, we can use the `Database::new` method to create a new database at a given path. If the path is `None`, then the database will be created in a temporary location. We can also use the `Database::clear` method to clear the database.

Importantly, if the `ConstructedBlock` does not have it's `hash` set, the block `number` will be used as it's unique identifier.


### License

// None yet
