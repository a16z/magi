#!/bin/sh
set -eou

DATADIR=/data/geth

if [ $NETWORK = "optimism-goerli" ]
then
    CHAIN_ID=420
    if [ ! -d $DATADIR ]
    then
        mkdir $DATADIR
        wget "https://storage.googleapis.com/oplabs-goerli-data/goerli-bedrock.tar" -P $DATADIR
        tar -xvf $DATADIR/goerli-bedrock.tar -C $DATADIR
    fi
elif [ $NETWORK = "base-goerli" ]
then
    CHAIN_ID=84531
    if [ ! -d $DATADIR ]
    then
        wget "https://raw.githubusercontent.com/base-org/node/main/goerli/genesis-l2.json" -O ./genesis-l2.json
        exec geth init --datadir=$DATADIR ./genesis-l2.json
    fi
else
    echo "Network not recognized. Available options optimism-goerli and base-goerli"
    exit 1
fi


echo $JWT_SECRET > jwtsecret.txt

echo "chain id"
echo $CHAIN_ID

exec geth \
  --datadir="$DATADIR" \
  --networkid="$CHAIN_ID" \
  --http \
  --http.corsdomain="*" \
  --http.vhosts="*" \
  --http.addr=0.0.0.0 \
  --http.port=8545 \
  --http.api=web3,debug,eth,txpool,net,engine \
  --syncmode=full \
  --gcmode=full \
  --nodiscover \
  --maxpeers=0 \
  --networkid=420 \
  --authrpc.vhosts="*" \
  --authrpc.addr=0.0.0.0 \
  --authrpc.port=8551 \
  --authrpc.jwtsecret=/jwtsecret.txt \
  --rollup.disabletxpoolgossip=true \
  $@
