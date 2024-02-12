## Infra

This contains a simple docker setup for running magi and op-geth.

## Running

Begin by copying `.env.default` to `.env`. You can set the network to sync to by changing the `NETWORK` value (supported options are optimism-goerli and base-goerli). Make sure to set the `L1_RPC_URL` value to a valid RPC URL for the L1 being used by the given network. If you are running in production, you may also want to set a secure `JWT_SECRET` value. You can create a new secret by running `openssl rand -hex 32`.

To run both magi and op-geth together, run `docker compose up`. To run just op-geth without magi for local development, run `COMPOSE_PROFILES=no-magi docker compose up`

## Troubleshooting
If you are getting `permission denied` errors when attempting to run `docker-compose`, try `sudo docker compose` instead. This is often required when running docker depending on how it was installed.
