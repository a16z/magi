FROM rust:1.68.0 as build
WORKDIR /magi
COPY ../ ./
RUN cargo build --release

FROM debian:buster-slim
RUN apt-get update && apt-get install -y libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /magi/target/release/magi /usr/local/bin
