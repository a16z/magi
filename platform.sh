#!/bin/bash

case $TARGETARCH in
    "amd64")
	echo "x86_64-unknown-linux-gnu" > /.platform
	echo "" > /.compiler 
	;;
    "arm64") 
	echo "aarch64-unknown-linux-gnu" > /.platform
	echo "gcc-aarch64-linux-gnu" > /.compiler
	;;
esac
