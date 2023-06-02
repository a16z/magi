## Specifications

### Driver

The [Driver](../src/driver/mod.rs) is the highest-level component in `magi`. It is responsible for driving the L2 chain forward by processing L1 blocks and deriving the L2 chain from them.

On instantiation, the [Driver](../src/driver/mod.rs) is provided with an instance of the [Engine API](#engine-api), [Pipeline](#derivation-pipeline), and [Config](#config).

Advancing the driver forward one block is then as simple as calling the [Driver::advance](../src/driver/mod.rs#L132) method as done in `magi`'s [main](../bin/magi.rs) binary.

Advancing the driver involves a few steps. First, the [Driver](../src/driver/mod.rs) will increment the [Pipeline](#derivation-pipeline) (as an iterator) to derive [PayloadAttributes](../src/engine/payload.rs). Then, the [Driver](../src/driver/mod.rs) will construct an [ExecutionPayload](../src/engine/payload.rs) that it can send through the [Engine API](#engine-api) as a `engine_newPayloadV1` request. Finally, the [ForkChoiceState](../src/engine/fork.rs) is updated by the driver, sending an `engine_forkchoiceUpdatedV1` request to the [Engine API](#engine-api).

At this point, `magi` has successfully advanced the L2 chain forward by one block.

### Engine API

The [EngineApi](../src/engine/mod.rs) exposes an interface for interacting with an external [execution client](https://ethereum.org/en/developers/docs/nodes-and-clients/#execution-clients), in our case [op-geth](https://github.com/ethereum-optimism/op-geth) or [op-reth](https://github.com/paradigmxyz/reth) (soon™). Notice, we cannot use [go-ethereum](https://github.com/ethereum/go-ethereum) as the execution client because Optimism's [execution client](https://github.com/ethereum-optimism/op-geth) requires a [minimal diff](https://op-geth.optimism.io/) to the [Engine API](https://github.com/ethereum/execution-apis/tree/main/src/engine).

To construct an [EngineApi](../src/engine/mod.rs) as done in the `magi` [main binary](../bin/magi.rs), we must provide it with a base url (port is optional, and by default `8551`) as well as a 256 bit, hex-encoded secret string that is used to authenticate requests to the node. This secret is configured on the execution node's side using the `--authrpc.jwtsecret` flag. See [start-op-geth.sh](../docker/start-op-geth.sh) for an example of how to configure and run an [op-geth](https://github.com/ethereum-optimism/op-geth) instance.

As mentioned in [Driver](#driver) section, the [Driver](../src/driver/mod.rs) uses the [EngineApi](../src/engine/mod.rs) to send constructed [ExecutionPayload](../src/engine/payload.rs) to the execution client using the [new_payload](../src/engine/api.rs#L187) method. It also updates the [ForkChoiceState](../src/engine/fork.rs) using the [forkchoice_updated](../src/engine/api.rs#L171) method.

Additionally, the [EngineApi](../src/engine/mod.rs) exposes a [get_payload](../src/engine/api.rs#L194) method to fetch the [ExecutionPayload](../src/engine/payload.rs) for a given block hash.

### Derivation Pipeline

As we mention in the [Driver](#driver) section, the [Derivation Pipeline](../src/derive/mod.rs) is responsible for much of `magi`'s functionality. It is used by the [Driver](#driver) to construct a [PayloadAttributes](../src/engine/payload.rs) from only an L1 RPC URL, passed through a [Config](#config) object.

When constructed, the [Pipeline](../src/derive/mod.rs) spawns an [L1 Chain Watcher](#l1-chain-watcher) and listens to the returned channel receivers for new L1 blocks and Deposit Transactions. It then uses its [stages](../src/derive/stages/mod.rs) as iterators to sequentially construct a [PayloadAttributes](../src/engine/payload.rs) from the L1 blocks and Deposit Transactions.

The Pipeline is broken up into [stages](../src/derive/stages/mod.rs) as follows.

#### Stages

##### Batcher Transactions

The [Batcher Transactions](../src/derive/stages/batcher_transactions.rs) stage pulls transactions from its configured channel receiver, passed down from the [Pipeline](../src/derive/mod.rs) parent. To construct a [Batcher Transaction](../src/derive/stages/batcher_transactions.rs) from the raw transaction data, it constructs [Frames](../src/derive/stages/batcher_transactions.rs) following the [Batch Submission Wire Format](https://github.com/ethereum-optimism/optimism/blob/develop/specs/derivation.md#batch-submission-wire-format) documented in the [Optimism Specs](https://github.com/ethereum-optimism/optimism/blob/develop/specs/README.md).

##### Channels

In the next stage, [Channels](../src/derive/stages/channels.rs), the [Batcher Transactions](../src/derive/stages/batcher_transactions.rs) is passed in and used as an iterator over the [Batcher Transaction](../src/derive/stages/batcher_transactions.rs) objects. The [Channels](../src/derive/stages/channels.rs) stage extracts [Frames](../src/derive/stages/batcher_transactions.rs) from the [Batcher Transaction](../src/derive/stages/batcher_transactions.rs) objects and places them in their corresponding [Channel](../src/derive/stages/channels.rs) objects. Since multiple channels can be built simultaneously, so-called `PendingChannel`s, the [Channel](../src/derive/stages/channels.rs) stage tracks if a channel is ready, and returns this when the Channel stage is called as an iterator.

Remember, since the [L1 Chain Watcher](#l1-chain-watcher) is spawned as a separate thread, it asynchronously feeds transactions and blocks over channels to the pipeline stages. As such, iterating over a stage like this one will return `None` until transactions are received from the [L1 Chain Watcher](#l1-chain-watcher) that can be split into frames and processed to fill up a full channel.

##### Batches

Next up, the [Batches](../src/derive/stages/batches.rs) stage iterates over the prior [Channel](../src/derive/stages/channels.rs) stage, decoding [Batch](../src/derive/stages/batches.rs) objects from the inner channel data. [Batch](../src/derive/stages/batches.rs) objects are RLP-decoded from the channel data following the [Batch Encoding Format](https://github.com/ethereum-optimism/optimism/blob/develop/specs/derivation.md#batch-format), detailed below.

For version 0, [Batch](../src/derive/stages/batches.rs) objects are encoded as follows:

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

Lastly, the [Pipeline](../src/derive/mod.rs) applies the [Attributes](../src/derive/stages/attributes.rs) stage to the previous [Batch](../src/derive/stages/batches.rs) stage, iterating over [Attributes](../src/derive/stages/attributes.rs).

In this step, the final [PayloadAttributes](../src/derive/stages/attributes.rs) object is constructed by combining the [Batch](../src/derive/stages/batches.rs) object data with its corresponding L1 Block, as well as applying system configuration values like the `suggested_fee_recipient`, `no_tx_pool`, and `gas_limit`.

### L1 Chain Watcher

The L1 chain watcher is responsible for watching L1 for new blocks with deposits and batcher transactions. `magi` spawns the L1 [`ChainWatcher`](../src/l1/mod.rs) in a separate thread and uses channels to communicate with the upstream consumers.

In `magi`'s case, the upstream consumers are the [`Pipeline`](../src/derive/mod.rs), which contains an instance of the [`ChainWatcher`](../src/l1/mod.rs) and passes the channel receivers into the pipeline [stages](../src/derive/stages/mod.rs).

When constructed in the [`Pipeline`](../src/derive/mod.rs), the [`ChainWatcher`](../src/l1/mod.rs) is provided with a [Config](../src/config/mod.rs) object that contains a critical config values for the L1 chain watcher. This includes:
- [L1 RPC Endpoint](../src/config/mod.rs#L41)
- [Deposit Contract Address](../src/config/mod.rs#L117)
- [Batch Sender Address](../src/config/mod.rs#L139)
- [Batch Inbox Address](../src/config/mod.rs#L115)

Note, when the `ChainWatcher` object is dropped, it will abort tasks associated with its handlers using [`tokio::task::JoinHandle::abort`](https://docs.rs/tokio/1.13.0/tokio/task/struct.JoinHandle.html#method.abort).

### Sync modes

Magi supports different [SyncModes](../src/config/mod.rs#L14) to sync the L2 chain. The sync mode can be set when calling the main binary with the `--sync-mode` flag. The following sync modes are supported:

- `full`: The full sync mode will sync the L2 chain from the genesis block. This is the default sync mode.
- `checkpoint`: The checkpoint sync mode will use a trusted L2 RPC endpoint to bootstrap the sync phase. It works by sending a forkchoice update request to the engine API to the latest block, which will make the execution client start the sync process using its p2p network, which is faster than syncing each block via L1. Once the execution client has synced, Magi takes over and starts the driver as normal.

### Config

The [Config](../src/config/mod.rs) object contains the system configuration for the `magi` node.

**Config**
- `l1_rpc_url`: The L1 RPC endpoint to use for the L1 chain watcher.
- `l2_rpc_url`: The L2 chain RPC endpoint
- `l2_engine_url`: The L2 chain engine API URL (see [Engine API](#engine-api)).
- `chain`: A `ChainConfig` object detailed below.
- `jwt_secret`: A hex-encoded secret string used to authenticate requests to the engine API.
- `checkpoint_sync_url`: The URL of the trusted L2 RPC endpoint to use for checkpoint syncing.
- `rpc_port`: The port to use for the Magi RPC server.

**ChainConfig**
- `network`: The network name.
- `chain_id`: The chain id.
- `l1_start_epoch`: The L1 block number to start the L1 chain watcher at.
- `l2_genesis`: The L2 genesis block.
- `system_config`: The initial system config struct.
- `batch_inbox`: The batch inbox address.
- `deposit_contract`: The L1 address of the deposit contract.
- `system_config_contract`: The L1 address of the system config contract.
- `max_channel_size`: The maximum byte size of all pending channels.
- `channel_timeout`: The max timeout for a channel (as measured by the frame L1 block number).
- `seq_window_size`: Number of L1 blocks in a sequence window.
- `max_seq_drift`: Maximum timestamp drift.
- `regolith_time`: Timestamp of the regolith hardfork.
- `blocktime`: The L2 blocktime.

The [ChainConfig](../src/config/mod.rs) contains default implementations for certain chains. For example, an `optimism-goerli` [ChainConfig](../src/config/mod.rs) instance can be created by calling `ChainConfig::optimism_goerli()`, and a `base-goerli` instance can be created by calling `ChainConfig::base_goerli()`.
