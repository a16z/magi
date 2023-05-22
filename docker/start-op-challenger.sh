#!/bin/sh
set -e

# how do we want to handle network-dependant configs here?
L2_OUTPUT_ORACLE=0xE6Dfba0953616Bacab0c9A8ecb3a9BBa77FC15c0

exec op-challenger \
    --l1-ws-endpoint ws://${EXECUTION_CLIENT}:8546 \
    --trusted-op-node-endpoint http://magi:${RPC_PORT} \
    --signer-key $CHALLENGER_SIGNER_KEY \
    --dispute-game-factory $CHALLENGER_DISPUTE_GAME_FACTORY \
    --l2-output-oracle $L2_OUTPUT_ORACLE \
    $@
