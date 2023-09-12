#!/bin/sh
set -e

DEVNET=""

if [ $NETWORK = "custom" ] || [ $NETWORK = "devnet" ]
then
    NETWORK="./rollup.json"
    if [ "$NETWORK" = "devnet" ]
    then
        DEVNET="--devnet"
    fi
fi

if [ $SYNC_MODE = "full" ]
then
    exec magi \
        --network $NETWORK \
        --jwt-secret $JWT_SECRET \
        --l1-rpc-url $L1_RPC_URL \
        --l2-rpc-url http://${EXECUTION_CLIENT}:8545 \
        --l2-engine-url http://${EXECUTION_CLIENT}:8551 \
        --rpc-port $RPC_PORT \
        $DEVNET \
        --sync-mode $SYNC_MODE
elif [ $SYNC_MODE = "checkpoint" ]
then
    exec magi \
        --network $NETWORK \
        --jwt-secret $JWT_SECRET \
        --l1-rpc-url $L1_RPC_URL \
        --l2-rpc-url http://${EXECUTION_CLIENT}:8545 \
        --l2-engine-url http://${EXECUTION_CLIENT}:8551 \
        --rpc-port $RPC_PORT \
        $DEVNET \
        --sync-mode $SYNC_MODE \
        --checkpoint-sync-url $CHECKPOINT_SYNC_URL \
        --checkpoint-hash $CHECKPOINT_HASH
else
    echo "Sync mode not recognized. Available options are full and checkpoint"
fi
