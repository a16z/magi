## Infra
This contains a simple docker setup for running op-geth on Optimism's Goerli testnet. This is a stripped down version of smartcontract's [simple-op-node](https://github.com/smartcontracts/simple-optimism-node) repo. All credit goes to him for making this setup so simple.

## Running 
Begin by copying `.env.default` to `.env`. This file contains the JWT secret that Magi will use to connect to op-geth. If you are running this in production, it is highly recommended to generate a new secret. You can create a new secret by running `openssl rand -hex 32`.

To begin op-geth, run `docker-compose up`. The first time you run this command, it will unpack and import the old Optimism Goerli state. This may take a while.
