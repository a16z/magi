#! /bin/bash

BEARER_TOKEN=${BEARER_TOKEN}
BASE_URL=${BASE_URL}

curl $BASE_URL:8551 \
    -X POST \
    -H `Authorization: Bearer $BEARER_TOKEN` \
    -H "Content-Type: application/json" \
    --data '{"method":"eth_chainId","params":[],"id":1,"jsonrpc":"2.0"}'

