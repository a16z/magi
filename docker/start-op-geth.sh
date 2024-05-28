#!/bin/sh
set -e

apk add zstd
apk add jq

DATADIR=/data/geth

if [ $NETWORK = "optimism" ]
then
    CHAIN_ID=10
    if [ ! -d $DATADIR ]
    then
        mkdir $DATADIR
        wget "https://datadirs.optimism.io/mainnet-bedrock.tar.zst" -P $DATADIR
        zstd -cd $DATADIR/mainnet-bedrock.tar.zst | tar xvf - -C $DATADIR
    fi
elif [ $NETWORK = "base" ]
then
    CHAIN_ID=8453
    if [ ! -d $DATADIR ]
    then
        mkdir $DATADIR
        wget "https://raw.githubusercontent.com/base-org/node/main/mainnet/genesis-l2.json" -O ./genesis-l2.json
        geth init --datadir=$DATADIR ./genesis-l2.json
    fi
elif [ "$NETWORK" = "optimism-sepolia" ]
then
    CHAIN_ID=11155420
    if [ ! -d "$DATADIR" ]
    then
        wget "https://storage.googleapis.com/oplabs-network-data/Sepolia/genesis.json" -O ./genesis-l2.json
        geth init --datadir=$DATADIR ./genesis-l2.json
    fi
elif [ $NETWORK = "base-sepolia" ]
then
    CHAIN_ID=84532
    if [ ! -d $DATADIR ]
    then
        wget "https://raw.githubusercontent.com/base-org/node/main/sepolia/genesis-l2.json" -O ./genesis-l2.json
        geth init --datadir=$DATADIR ./genesis-l2.json
    fi
elif [ $NETWORK = "custom" ] || [ $NETWORK = "devnet" ]
then
    CHAIN_ID=$(jq '.config.chainId' ./genesis-l2-attached.json)
    
    if [ ! -d $DATADIR ]
    then
        mkdir $DATADIR
        geth init --datadir=$DATADIR ./genesis-l2-attached.json
    fi
else
    echo "Network not recognized. Available options are optimsim, optimism-sepolia, base, base-sepolia, custom"
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
  --authrpc.vhosts="*" \
  --authrpc.addr=0.0.0.0 \
  --authrpc.port=8551 \
  --authrpc.jwtsecret=/jwtsecret.txt \
  --rollup.disabletxpoolgossip=true \
  --snapshot=false 
  $@
