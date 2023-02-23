<img align="right" width="150" height="150" top="100" src="./assets/magi.png">

# magi • [![tests](https://github.com/a16z/magi/actions/workflows/test.yml/badge.svg?label=tests)](https://github.com/a16z/magi/actions/workflows/test.yml) ![license](https://img.shields.io/github/license/a16z/magi?label=license)

`magi` (pronounced may-jai) is an Optimism full node implemented in pure Rust.


### Features

- [ ] Base Chain Watcher
    - [x] Transaction Watcher
    - [x] Block Watcher
    - [ ] Receipt Watcher
- [ ] Derivation Pipeline
    - [x] Batcher Transaction
    - [x] Channels
    - [ ] Batches
        - [x] Basic Setup
        - [ ] Pruning
    - [ ] Paylaod Attributes
        - [x] Basic Setup
        - [x] Attributes Deposited Transactions
            - [x] Basic Setup
            - [x] Handle Sequence Numbers
        - [ ] User Deposited Transactions
- [ ] Geth Driver
    - [ ] Engine API Bindings
    - [ ] Driver Loop
- [ ] Backend DB
    - [ ] Progress Tracker

### Usage

_Prerequisites: Install rust and cargo with `curl https://sh.rustup.rs -sSf | sh`_

Run the main binary with `cargo run --bin magi`


### License

// None yet
