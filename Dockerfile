FROM --platform=linux/amd64 rust:latest as build
WORKDIR /magi

COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src/bin && echo "fn main() {}" > src/bin/dummy.rs
RUN cargo build --release --config net.git-fetch-with-cli=true --bin dummy

COPY ./ ./
RUN cargo build --release --config net.git-fetch-with-cli=true

FROM --platform=linux/amd64 debian:buster-slim
RUN apt-get update && apt-get install -y libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /magi/target/release/magi /usr/local/bin
