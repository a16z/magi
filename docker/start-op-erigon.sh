#!/bin/sh
set -e

DATADIR=/data/erigon

echo $JWT_SECRET > jwtsecret.txt

exec erigon \
    --datadir=$DATADIR \
    --private.api.addr=localhost:9090 \
    --http.addr=0.0.0.0 \
    --http.port=8545 \
    --http.corsdomain="*" \
    --http.vhosts="*" \
    --authrpc.addr=0.0.0.0 \
    --authrpc.port=8551 \
    --authrpc.vhosts="*" \
    --authrpc.jwtsecret=/jwtsecret.txt \
    --rollup.sequencerhttp="https://sepolia.optimism.io" \
