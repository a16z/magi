## Magi

[![build](https://github.com/a16z/magi/actions/workflows/test.yml/badge.svg)](https://github.com/a16z/magi/actions/workflows/test.yml) [![license: AGPL v3](https://img.shields.io/badge/License-AGPL_v3-blue.svg)](https://opensource.org/license/agpl-v3/) [![chat](https://img.shields.io/badge/chat-telegram-blue)](https://t.me/+6zrIsnaLO0hjNmZh)

Magi is an Optimism rollup node written in Rust. It is intended to perform the same functionality as op-node, and works with execution clients such as op-geth. As an independent implementation of the rollup node, we hope to increase the safety and liveliness of the entire op-stack ecosystem. Magi is still new, so we expect to find some bugs in the coming months. For criticial infrastructure, we recommend using op-node.

## Running

For convenience, we have provides simple docker setup to run Magi and op-geth together. This guide assumes you have both `docker` and `git` installed on your machine.

Start by cloning the Magi repository and entering the docker subdirectory
```sh
git clone git@github.com:a16z/magi.git && cd magi/docker
```

Next copy `.env.default` to `.env`
```sh
cp .env.default .env
```

In the `.env` file, modify the `L1_RPC_URL` field to contain a valid Ethereum RPC. For the Optimism and Base testnets, this must be a Goerli RPC URL. This RPC can either be from a local node, or a provider such as Alchemy or Infura. 

By default, the `NETWORK` field in `.env` is `optimism-goerli`, however `base-goerli` is also supported.

Start the docker containers
```sh
docker compose up -d
```

If the previous step yields an error mentionign permissions. Try running the command with `sudo`.

The docker setup contains a Grafana dashboard. To view sync progress, you can check the dashboard at `http://localhost:3000`. Alternatively, you can view Magi's logs by running `docker logs magi --follow`.

## Contributing

All contributions to Magi are welcome. Before opening a PR, please submit an issue detailing the bug or feature. When opening a PR, please ensure that your contribution builds on the nightly rust toolchain, has been linted with `cargo fmt`, passes `cargo clippy`, and contains tests when applicable.

## Disclaimer

_This code is being provided as is. No guarantee, representation or warranty is being made, express or implied, as to the safety or correctness of the code. It has not been audited and as such there can be no assurance it will work as intended, and users may experience delays, failures, errors, omissions or loss of transmitted information. Nothing in this repo should be construed as investment advice or legal advice for any particular facts or circumstances and is not meant to replace competent counsel. It is strongly advised for you to contact a reputable attorney in your jurisdiction for any questions or concerns with respect thereto. a16z is not liable for any use of the foregoing, and users should proceed with caution and use at their own risk. See a16z.com/disclosures for more info._
