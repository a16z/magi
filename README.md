<img align="right" width="150" height="150" top="100" src="./assets/magi.png">

# magi • [![tests](https://github.com/a16z/magi/actions/workflows/test.yml/badge.svg?label=tests)](https://github.com/a16z/magi/actions/workflows/test.yml) ![license](https://img.shields.io/github/license/a16z/magi?label=license)

`magi` (pronounced may-jai) is an Optimism full node implemented in pure Rust.

### Getting Started

_Prerequisites: Install rust and cargo with `curl https://sh.rustup.rs -sSf | sh`_

Install the latest version of `magi` with

```bash
cargo install magi
```

Alternatively, you can clone the repo and run `cargo build --release` to build a release binary that you can then run with `./target/release/magi`. Cargo also allows you to run `cargo run --release`, building and executing the binary in one step.

To run `magi`'s test suite, you can run `cargo test --all`. Tests are named with respect to the modules they test against, located inside the [tests](./tests) directory. There are also additional unit tests written inline with some modules.

### Specifications

#### L1 Chain Watcher

The L1 chain watcher is responsible for watching L1 for new blocks with deposits and batcher transactions. `magi` spawns the L1 [`ChainWatcher`](./src/l1/mod.rs) in a separate thread and uses channels to communicate with the upstream consumers.

In `magi`'s case, the upstream consumers are the [`Pipeline`](./src/derive/mod.rs), which contains an instance of the [`ChainWatcher`](./src/l1/mod.rs) and passes the channel receivers into the pipeline [stages](./src/derive/stages/mod.rs).

When constructed in the [`Pipeline`](./src/derive/mod.rs), the [`ChainWatcher`](./src/l1/mod.rs) is provided with a [Config](./src/config.rs) object that contains a critical config values for the L1 chain watcher. This includes:
- [L1 RPC Endpoint](./src/config/mod.rs#L11)
- [Deposit Contract Address](./src/config/mod.rs#L32)
- [Batch Sender Address](./src/config/mod.rs#L30)
- [Batch Inbox Address](./src/config/mod.rs#L30)

Note, when the `ChainWatcher` object is dropped, it will abort tasks associated with its handlers using [`tokio::task::JoinHandle::abort`](https://docs.rs/tokio/1.13.0/tokio/task/struct.JoinHandle.html#method.abort).

#### Derivation Pipeline

The derivation pipeline is responsible for deriving the canonical L2 chain from the L1 chain.

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

### Feature Requests

- [ ] Introduce a System Config Watcher that watches for changes to the system config on new L1 blocks. This should be handled in the L1 Chain Watcher and be run for each new block. Note: if the system config changes, any batched transactions, in the **entire** block, will be affected by the system config change.
- [ ] In the [Driver](./src/driver/mod.rs), we should be writing to the [Backend DB](./src/backend/mod.rs) as we process blocks. This allows for persisting L2 chain state on disk and optionally allows for restarting the node without having to re-process all of the blocks.
- [ ] In the [Backend DB](./src/backend/mod.rs), the `ConstructedBlock` type should match, or at least implement coercions to/from, the [Driver](./src/driver/mod.rs) output type.
- [ ] Subscribe to P2P Gossip on the configured L2 P2P Network. This will allow us to receive new blocks from other nodes on the network.

### License

// None yet
