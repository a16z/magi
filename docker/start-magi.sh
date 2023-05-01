#!/bin/sh
set -e

if [ $EXECUTION_CLIENT = "op-geth" ]
then
    echo "using op-geth"
elif [ $EXECUTION_CLIENT = "op-erigon" ]
then
    echo "using op-erigon"
else
    echo "Execution client not recongnized. Must use op-geth or op-erigon"
    exit 1
fi

if [ $SYNC_MODE = "full" ]
then
    echo "starting in full sync mode"
elif [ $SYNC_MODE = "checkpoint" ]
then
    echo "starting in checkpoint sync mode"
else
    echo "Sync mode not recognized. Must use full or checkpoint"
    exit 1
fi

exec magi \
    --network $NETWORK \
    --jwt-secret $JWT_SECRET \
    --l1-rpc-url $L1_RPC_URL \
    --l2-rpc-url http://${EXECUTION_CLIENT}:8545 \
    --l2-engine-url http://${EXECUTION_CLIENT}:8551 \
    --l2-trusted-rpc-url $TRUSTED_L2_RPC_URL \
    --rpc-port $RPC_PORT \
    --sync-mode $SYNC_MODE \
    --checkpoint-hash $CHECKPOINT_HASH
