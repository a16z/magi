#!/bin/sh
set -eou

if [ ! -d $BEDROCK_DATADIR ]
then
    echo "extracting datadir"
    mkdir $BEDROCK_DATADIR
    wget "https://storage.googleapis.com/oplabs-goerli-data/goerli-bedrock.tar" -P $BEDROCK_DATADIR
    tar -xvf $BEDROCK_DATADIR/goerli-bedrock.tar
else
    echo "datadir already exists"
fi

echo $JWT_SECRET > jwtsecret.txt


exec geth \
  --datadir="$BEDROCK_DATADIR" \
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
  --rollup.sequencerhttp="$BEDROCK_SEQUENCER_HTTP" \
  --rollup.disabletxpoolgossip=true \
  $@
