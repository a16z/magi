#! /bin/bash

echo Posting to $1
echo Posting data '{"jsonrpc":"2.0","method":"engine_getPayloadV1","params":["0x0000000021f32cc1"],"id":1}'

curl -X POST --data '{"jsonrpc":"2.0","method":"engine_getPayloadV1","params":["0x0000000021f32cc1"],"id":1}' $1
