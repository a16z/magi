## Specifications

### Driver

The [Driver](./src/driver/mod.rs) is the highest-level component in `magi`. It is responsible for driving the L2 chain forward by processing L1 blocks and deriving the L2 chain from them.

On instantiation, the [Driver](./src/driver/mod.rs) is provided with an instance of the [Engine API](#engine-api), [Pipeline](#derivation-pipeline), and [Config](#config).

Advancing the driver forward one block is then as simple as calling the [Driver::advance](./src/driver/mod.rs#45) method as done in `magi`'s [main](./src/main.rs) binary.

Advancing the driver involves a few steps. First, the [Driver](./src/driver/mod.rs) will increment the [Pipeline](#derivation-pipeline) (as an iterator) to derive [PayloadAttributes](./src/engine/payload.rs). Then, the [Driver](./src/driver/mod.rs) will construct an [ExecutionPayload](./src/engine/payload.rs) that it can send through the [Engine API](#engine-api) as a `engine_newPayloadV1` request. Finally, the [ForkChoiceState](./src/engine/fork.rs) is updated by the driver, sending an `engine_forkchoiceUpdatedV1` request to the [Engine API](#engine-api).

At this point, `magi` has successfully advanced the L2 chain forward by one block, and the [Driver](./src/driver/mod.rs) should store the L2 Block in the [Backend DB](#backend-db).

### Engine API

The [EngineApi](./src/engine/mod.rs) exposes an interface for interacting with an external [execution client](https://ethereum.org/en/developers/docs/nodes-and-clients/#execution-clients), in our case [op-geth](https://github.com/ethereum-optimism/op-geth) or [op-reth](https://github.com/rkrasiuk/op-reth) (soon™). Notice, we cannot use [go-ethereum](https://github.com/ethereum/go-ethereum) as the execution client because Optimism's [execution client](https://github.com/ethereum-optimism/op-geth) requires a [minimal diff](https://op-geth.optimism.io/) to the [Engine API](https://github.com/ethereum/execution-apis/tree/main/src/engine).

To construct an [EngineApi](./src/engine/mod.rs) as done in the `magi` [main binary](./src/main.rs), we must provide it with a base url (port is optional, and by default `8551`) as well as a 256 bit, hex-encoded secret string that is used to authenticate requests to the node. This secret is configured on the execution node's side using the `--authrpc.jwtsecret` flag. See [start-op-geth.sh](./scripts/start-op-geth.sh) for an example of how to configure and run an [op-geth](https://github.com/ethereum-optimism/op-geth) instance.

As mentioned in [Driver](#driver) section, the [Driver](./src/driver/mod.rs) uses the [EngineApi](./src/engine/mod.rs) to send constructed [ExecutionPayload](./src/engine/payload.rs) to the execution client using the [new_payload](./src/engine/api.rs) method. It also updates the [ForkChoiceState](./src/engine/fork.rs) using the [forkchoice_updated](./src/engine/api.rs) method.

Additionally, the [EngineApi](./src/engine/mod.rs) exposes a [get_payload](./src/engine/api.rs) method to fetch the [ExecutionPayload](./src/engine/payload.rs) for a given block hash.

### Derivation Pipeline

As we mention in the [Driver](#driver) section, the [Derivation Pipeline](./src/derive/mod.rs) is responsible for much of `magi`'s functionality. It is used by the [Driver](#driver) to construct a [PayloadAttributes](./src/engine/payload.rs) from only an L1 RPC URL, passed through a [Config](#config) object.

When constructed, the [Pipeline](./src/derive/mod.rs) spawns an [L1 Chain Watcher](#l1-chain-watcher) and listens to the returned channel receivers for new L1 blocks and Deposit Transactions. It then uses its [stages](./src/derive/stages/mod.rs) as iterators to sequentially construct a [PayloadAttributes](./src/engine/payload.rs) from the L1 blocks and Deposit Transactions.

The Pipeline is broken up into [stages](./src/derive/stages/mod.rs) as follows.

#### Stages

##### Batcher Transactions

The [Batcher Transactions](./src/derive/stages/batcher.rs) stage pulls transactions from its configured channel receiver, passed down from the [Pipeline](./src/derive/mod.rs) parent. To construct a [BatcherTransaction](./src/derive/stages/batcher_transactions.rs) from the raw transaction data, it constructs [Frames](./src/derive/stages/batcher_transactions.rs) following the [Batch Submission Wire Format](https://github.com/ethereum-optimism/optimism/blob/develop/specs/derivation.md#batch-submission-wire-format) documented in the [Optimism Specs](https://github.com/ethereum-optimism/optimism/blob/develop/specs/README.md).

##### Channels

In the next stage, [Channels](./src/derive/stages/channels.rs), the [BatcherTransactions](./src/derive/stages/batcher.rs) is passed in and used as an iterator over the [BatcherTransaction](./src/derive/stages/batcher.rs) objects. The [Channels](./src/derive/stages/channels.rs) stage extracts [Frames](./src/derive/stages/batcher.rs) from the [BatcherTransaction](./src/derive/stages/batcher.rs) objects and places them in their corresponding [Channel](./src/derive/stages/channels.rs) objects. Since multiple channels can be built simultaneously, so-called `PendingChannel`s, the [Channel](./src/derive/stages/channels.rs) stage tracks if a channel is ready, and returns this when the Channel stage is called as an iterator.

Remember, since the [L1 Chain Watcher](#l1-chain-watcher) is spawned as a separate thread, it asynchronously feeds transactions and blocks over channels to the pipeline stages. As such, iterating over a stage like this one will return `None` until transactions are received from the [L1 Chain Watcher](#l1-chain-watcher) that can be split into frames and processed to fill up a full channel.

##### Batches

Next up, the [Batches](./src/derive/stages/batches.rs) stage iterates over the prior [Channel](./src/derive/stages/channels.rs) stage, decoding [Batch](./src/derive/stages/batches.rs) objects from the inner channel data. [Batch](./src/derive/stages/batches.rs) objects are RLP-decoded from the channel data following the [Batch Encoding Format](https://github.com/ethereum-optimism/optimism/blob/develop/specs/derivation.md#batch-format), detailed below.

For version 0, [Batch](./src/derive/stages/batches.rs) objects are encoded as follows:

```golang
rlp_encode([parent_hash, epoch_number, epoch_hash, timestamp, transaction_list])
```

In this encoding,
- `rlp_encode` encodes batches following the RLP format
- `parent_hash` is the block hash of the previous L2 block
- `epoch_number`is the number of the L1 block corresponding to the sequencing epoch of the L2 block
- `epoch_hash` is the hash of the L1 block corresponding to the sequencing epoch of the L2 block
- `timestamp` is the timestamp of the L2 block
- `transaction_list` is an RLP-encoded list of EIP-2718 encoded transactions.

##### Attributes

Lastly, the [Pipeline](./src/derive/mod.rs) applies the [Attributes](./src/derive/stages/attributes.rs) stage to the previous [Batch](./src/derive/stages/batches.rs) stage, iterating over [Attributes](./src/derive/stages/attributes.rs).

In this step, the final [PayloadAttributes](./src/derive/stages/attributes.rs) object is constructed by combining the [Batch](./src/derive/stages/batches.rs) object data with its corresponding L1 Block, as well as applying system configuration values like the `suggested_fee_recipient`, `no_tx_pool`, and `gas_limit`.

### L1 Chain Watcher

The L1 chain watcher is responsible for watching L1 for new blocks with deposits and batcher transactions. `magi` spawns the L1 [`ChainWatcher`](./src/l1/mod.rs) in a separate thread and uses channels to communicate with the upstream consumers.

In `magi`'s case, the upstream consumers are the [`Pipeline`](./src/derive/mod.rs), which contains an instance of the [`ChainWatcher`](./src/l1/mod.rs) and passes the channel receivers into the pipeline [stages](./src/derive/stages/mod.rs).

When constructed in the [`Pipeline`](./src/derive/mod.rs), the [`ChainWatcher`](./src/l1/mod.rs) is provided with a [Config](./src/config.rs) object that contains a critical config values for the L1 chain watcher. This includes:
- [L1 RPC Endpoint](./src/config/mod.rs#L11)
- [Deposit Contract Address](./src/config/mod.rs#L32)
- [Batch Sender Address](./src/config/mod.rs#L30)
- [Batch Inbox Address](./src/config/mod.rs#L30)

Note, when the `ChainWatcher` object is dropped, it will abort tasks associated with its handlers using [`tokio::task::JoinHandle::abort`](https://docs.rs/tokio/1.13.0/tokio/task/struct.JoinHandle.html#method.abort).

### Backend DB

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

Importantly, if the `ConstructedBlock` does not have its `hash` set, the block `number` will be used as its unique identifier.

### Config

The [Config](./src/config/mod.rs) object contains the system configuration for the `magi` node.

**Config**
- `l1_rpc`: The L1 RPC endpoint to use for the L1 chain watcher.
- `max_channels`: The maximum number of channels to use in the [Pipeline](./src/derive/mod.rs).
- `max_timeout`: The maximum timeout for a channel, measured by the frame's corresponding L1 block number.
- `chain`: A `ChainConfig` object detailed below.

**ChainConfig**
- `l1_start_epoch`: The L1 block number to start the L1 chain watcher at.
- `l2_genesis`: The L2 genesis block.
- `batch_sender`: The L1 address of the batch sender.
- `batch_inbox`: The batch inbox address.
- `deposit_contract`: The L1 address of the deposit contract.
- `blocktime`: The L2 blocktime.

The [ChainConfig](./src/config/mod.rs) contains default implementations for certain chains. For example, a `goerli` [ChainConfig](./src/config/mod.rs) instance can be created by calling `ChainConfig::goerli()`.
