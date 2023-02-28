#! /bin/bash

echo Posting to $BASE_URL:8545
echo Posting method: eth_syncing

curl $BASE_URL:8545 \
    -X POST \
    -H "Content-Type: application/json" \
    --data '{"jsonrpc":"2.0","method":"eth_syncing","params":[],"id":1}'
