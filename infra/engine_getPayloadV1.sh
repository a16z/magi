#! /bin/bash

# Construct the bearer token
export BEARER_TOKEN=$(jwtauth c $JWT_SECRET)

echo Posting to $BASE_URL:8551
echo Posting data "{\"jsonrpc\":\"2.0\",\"method\":\"engine_getPayloadV1\",\"params\":[\"$1\"],\"id\":1}"
echo Using bearer token $BEARER_TOKEN

curl $BASE_URL:8551 \
    -X POST \
    -H "Authorization: Bearer $BEARER_TOKEN" \
    -H "Content-Type: application/json" \
    --data "{\"jsonrpc\":\"2.0\",\"method\":\"engine_getPayloadV1\",\"params\":[\"$1\"],\"id\":1}"

