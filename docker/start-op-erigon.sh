#!/bin/sh
set -e

DATADIR=/data/erigon

if [ $NETWORK = "optimism-goerli" ]
then
    if [ ! -d $DATADIR ]
    then
        mkdir $DATADIR
        wget "https://backup.goerli.op-erigon.testinprod.io" -O $DATADIR/backup.tar.gz
        tar -xvf $DATADIR/backup.tar.gz -C $DATADIR
    fi
else
    echo "Network not recognized. Available option is optimism-goerli. Use op-geth for base-goerli"
    exit 1
fi

echo $JWT_SECRET > jwtsecret.txt

erigon \
    --datadir=$DATADIR \
    --private.api.addr=localhost:9090 \
    --http.addr=0.0.0.0 \
    --http.port=8545 \
    --http.corsdomain="*" \
    --http.vhosts="*" \
    --authrpc.addr=0.0.0.0 \
    --authrpc.port=8551 \
    --authrpc.vhosts="*" \
    --externalcl \
    --authrpc.jwtsecret=/jwtsecret.txt \
    --rollup.sequencerhttp="https://goerli.optimism.io" \
    --nodiscover
