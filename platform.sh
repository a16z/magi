#!/bin/bash

case $TARGETARCH in
    "amd64")
	echo "x86_64-unknown-linux-musl" > /.platform
	echo "musl-tools gcc-x86-64-linux-gnu" > /.compiler 
    echo "-C linker=x86_64-linux-gnu-gcc" > /.rustflags
	;;
    "arm64") 
	echo "aarch64-unknown-linux-gnu" > /.platform
	echo "gcc-aarch64-linux-gnu" > /.compiler
    echo "" > /.rustflags
	;;
esac
