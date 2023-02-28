#! /bin/bash

curl $BASE_URL:8545 \
    -X POST \
    -H "Content-Type: application/json" \
    --data '{"method":"eth_chainId","params":[],"id":1,"jsonrpc":"2.0"}'

