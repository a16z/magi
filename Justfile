set positional-arguments
set dotenv-load := true

alias t := test
alias b := build-all
alias l := lint
alias f := fmt

# default recipe to display help information
default:
  @just --list

# Builds all docker images
build-all:
	docker buildx build --platform linux/arm64,linux/amd64 -t a16zcrypto/magi --push .

# Builds local magi docker images
build-local:
	docker buildx build -t noah7545/magi --load .

# Pulls all docker images
pull:
	cd docker && docker compose pull

# Cleans all docker images
clean:
  cd docker && docker compose down -v --remove-orphans

# Composes docker
run:
  cd docker && docker compose up

# Composes docker with local images
run-local:
  just build-local && cd docker && docker compose up

# Runs op-geth with docker
run-geth:
  cd docker && COMPOSE_PROFILES=op-geth docker compose up

# Runs op-erigon with docker
run-erigon:
  cd docker && COMPOSE_PROFILES=op-erigon docker compose up

# Run all tests
tests: test test-docs

# Test for the native target with all features
test *args='':
  cargo nextest run --all --all-features $@

# Lint for all available targets
lint: lint-native lint-docs

# Fixes and checks the formatting
fmt: fmt-native-fix fmt-native-check

# Fixes the formatting
fmt-native-fix:
  cargo +nightly fmt --all

# Check the formatting
fmt-native-check:
  cargo +nightly fmt --all -- --check

# Lints
lint-native: fmt-native-check
  cargo +nightly clippy --all --all-features --all-targets -- -D warnings

# Lint the Rust documentation
lint-docs:
  RUSTDOCFLAGS="-D warnings" cargo doc --all --no-deps --document-private-items 

# Test the Rust documentation
test-docs:
  cargo test --doc --all --locked
