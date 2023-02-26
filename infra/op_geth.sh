#!/bin/sh
set -eou

# Wait for the Bedrock flag for this network to be set.
while [ ! -f /shared/initialized.txt ]; do
  echo "Waiting for Bedrock node to initialize..."
  sleep 60
done

# Start op-geth.
exec geth \
  --datadir="$BEDROCK_DATADIR" \
  --http \
  --http.corsdomain="*" \
  --http.vhosts="*" \
  --http.addr=0.0.0.0 \
  --http.port=8545 \
  --http.api=web3,debug,eth,txpool,net,engine \
  --engine-rpc-enabled \
  --ws \
	--ws.addr=0.0.0.0 \
	--ws.port=8546 \
	--ws.origins="*" \
	--ws.api=debug,eth,txpool,net,engine \
  --metrics \
  --metrics.influxdb \
  --metrics.influxdb.endpoint=http://influxdb:8086 \
  --metrics.influxdb.database=l2geth \
  --syncmode=full \
  --gcmode="$NODE_TYPE" \
  --nodiscover \
  --maxpeers=0 \
  --networkid=420 \
  --authrpc.vhosts="*" \
  --authrpc.addr=0.0.0.0 \
  --authrpc.port=8551 \
  --authrpc.jwtsecret=/shared/jwt.txt \
  --rollup.sequencerhttp="$BEDROCK_SEQUENCER_HTTP" \
  --rollup.disabletxpoolgossip=true \
  --rollup.historicalrpc=http://l2geth:8545 \
  $@
