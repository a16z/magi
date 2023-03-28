FROM --platform=$BUILDPLATFORM rust:latest as build
WORKDIR /magi
ARG TARGETARCH

COPY ./platform.sh .
RUN ./platform.sh
RUN rustup target add $(cat /.platform) 
RUN apt-get update && apt-get install -y $(cat /.compiler)
ENV CC_x86_64_unknown_linux_musl=x86_64-linux-gnu-gcc

COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src/bin && echo "fn main() {}" > src/bin/dummy.rs
RUN RUSTFLAGS="$(cat /.rustflags)" cargo build --release --config net.git-fetch-with-cli=true --target $(cat /.platform) --bin dummy

COPY ./ ./
RUN RUSTFLAGS="$(cat /.rustflags)" cargo build --release --config net.git-fetch-with-cli=true --target $(cat /.platform)
RUN cp /magi/target/$(cat /.platform)/release/magi /magi/magi

FROM debian:buster-slim
RUN apt-get update && apt-get install -y libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /magi/magi /usr/local/bin
