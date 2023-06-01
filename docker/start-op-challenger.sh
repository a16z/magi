#!/bin/sh
set -e

if [ $NETWORK = "optimism-goerli" ]
then
    DISPUTE_GAME_FACTORY=0x000000000000000000000000000000000000dEaD
    L2_OUTPUT_ORACLE=0xE6Dfba0953616Bacab0c9A8ecb3a9BBa77FC15c0
elif [ $NETWORK = "base-goerli" ]
then 
    DISPUTE_GAME_FACTORY=0x000000000000000000000000000000000000dEaD
    L2_OUTPUT_ORACLE=0x2A35891ff30313CcFa6CE88dcf3858bb075A2298
else
    echo "Network not recognized. Available options are optimism-goerli and base-goerli"
    exit 1
fi

if [ $OP_CHALLENGER_MODE = "listen-only" ] 
then
    exec op-challenger \
        --l1-ws-endpoint ${L1_WS_RPC_URL} \
        --trusted-op-node-endpoint http://magi:${RPC_PORT} \
        --dispute-game-factory $DISPUTE_GAME_FACTORY \
        --l2-output-oracle $L2_OUTPUT_ORACLE \
        --mode listen-only \
        -vv
elif [ $OP_CHALLENGER_MODE = "listen-and-respond" ]
then
    exec op-challenger \
        --l1-ws-endpoint ${L1_WS_RPC_URL} \
        --trusted-op-node-endpoint http://magi:${RPC_PORT} \
        --signer-key $OP_CHALLENGER_SIGNER_KEY \
        --dispute-game-factory $DISPUTE_GAME_FACTORY \
        --l2-output-oracle $L2_OUTPUT_ORACLE \
        --mode listen-and-respond \
        -vv
else
    echo "Challenger mode not recognized. Available options are listen-only and listen-and-respond"
fi
