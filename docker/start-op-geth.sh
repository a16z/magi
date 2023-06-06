#!/bin/sh
set -e

DATADIR=/data/geth

if [ $NETWORK = "optimism" ]
then
    CHAIN_ID=10
    if [ ! -d $DATADIR ]
    then
        mkdir $DATADIR
        wget "https://storage.googleapis.com/oplabs-mainnet-data/mainnet-bedrock.tar" -P $DATADIR
        tar -xvf $DATADIR/mainnet-bedrock.tar -C $DATADIR
    fi
elif [ $NETWORK = "optimism-goerli" ]
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
    echo "Network not recognized. Available options are optimism-goerli and base-goerli"
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
  --http.api=web3,debug,eth,txpool,net,engine,admin \
  --syncmode=full \
  --gcmode=full \
  --networkid=420 \
  --authrpc.vhosts="*" \
  --authrpc.addr=0.0.0.0 \
  --authrpc.port=8551 \
  --authrpc.jwtsecret=/jwtsecret.txt \
  --rollup.disabletxpoolgossip=true \
  $@
