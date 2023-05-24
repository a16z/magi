#!/bin/sh
set -e

# how do we want to handle network-dependant configs here?
DISPUTE_GAME_FACTORY=0x5a20cD16d4E51D3B2A6b2Ab460Ac735Fe421b820
L2_OUTPUT_ORACLE=0xE6Dfba0953616Bacab0c9A8ecb3a9BBa77FC15c0

echo "Starting op-challenger"

exec op-challenger \
    -vv \
    --l1-ws-endpoint ${L1_WS_RPC_URL}:8546 \
    --trusted-op-node-endpoint http://magi:${RPC_PORT} \
    --signer-key $CHALLENGER_SIGNER_KEY \
    --dispute-game-factory $DISPUTE_GAME_FACTORY \
    --l2-output-oracle $L2_OUTPUT_ORACLE \
    $@
