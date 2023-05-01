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

if [ $SYNC_MODE = "full" ]
then
    exec magi \
        --network $NETWORK \
        --jwt-secret $JWT_SECRET \
        --l1-rpc-url $L1_RPC_URL \
        --l2-rpc-url http://${EXECUTION_CONTAINER}:8545 \
        --l2-engine-url http://${EXECUTION_CONTAINER}:8551 \
        --rpc-port $RPC_PORT \
        --sync-mode full
elif [ $SYNC_MODE = "checkpoint" ]
then
    exec magi \
        --network $NETWORK \
        --jwt-secret $JWT_SECRET \
        --l1-rpc-url $L1_RPC_URL \
        --l2-rpc-url http://${EXECUTION_CONTAINER}:8545 \
        --l2-engine-url http://${EXECUTION_CONTAINER}:8551 \
        --l2-trusted-rpc-url $TRUSTED_L2_RPC_URL \
        --rpc-port $RPC_PORT \
        --sync-mode checkpoint \
        --checkpoint-hash $CHECKPOINT_HASH
else
    echo "Sync mode not recognized. Must use `full` or `checkpoint`"
    exit 1
fi
