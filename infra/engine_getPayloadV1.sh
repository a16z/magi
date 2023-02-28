#! /bin/bash


BEARER_TOKEN=${BEARER_TOKEN}
BASE_URL=${BASE_URL}

echo Posting to $BASE_URL:8551
echo Posting data '{"jsonrpc":"2.0","method":"engine_getPayloadV1","params":["0x0000000021f32cc1"],"id":1}'
echo Using bearer token $BEARER_TOKEN

curl $BASE_URL:8551 \
    -X POST \
    -H "Authorization: Bearer $BEARER_TOKEN" \
    -H "Content-Type: application/json" \
    --data '{"jsonrpc":"2.0","method":"engine_getPayloadV1","params":["0x0000000021f32cc1"],"id":1}'
