#! /bin/bash

echo Posting to $1
echo Posting method: eth_syncing

curl -X POST -H "Content-Type: application/json" --data '{"jsonrpc":"2.0","method":"eth_syncing","params":[],"id":1}' $1
