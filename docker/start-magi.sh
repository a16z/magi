#!/bin/sh
set -eou

DATADIR=/data/magi

exec magi \
    --network $NETWORK \
    --jwt-secret $JWT_SECRET \
    --l1-rpc-url $L1_RPC_URL \
    --l2-rpc-url http://op-geth:8545 \
    --l2-engine-url http://op-geth:8551 \
    --data-dir $DATADIR \
    --sync-mode full \
