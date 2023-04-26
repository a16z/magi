#!/bin/sh
set -e

if [ -n "$USE_OP_GETH" ]
then
    EXECUTION_CONTAINER=op-geth
elif [ -n "$USE_OP_ERIGON" ]
then
    EXECUTION_CONTAINER=op-erigon
else
    echo "Execution client not recongnized. Must use op-geth or op-erigon"
    exit 1
fi

exec magi \
    --network $NETWORK \
    --jwt-secret $JWT_SECRET \
    --l1-rpc-url $L1_RPC_URL \
    --l2-rpc-url http://${EXECUTION_CONTAINER}:8545 \
    --l2-engine-url http://${EXECUTION_CONTAINER}:8551 \
    --rpc-port $RPC_PORT \
    --sync-mode full
