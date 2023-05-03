#!/bin/sh
set -e

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
