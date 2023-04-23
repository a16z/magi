.PHONY: build-all build-local pull clean run run-local run-geth run-erigon

build-all:
	docker buildx build --platform linux/arm64,linux/amd64 -t a16zcrypto/magi --push .

build-local:
	docker buildx build -t noah7545/magi --load .

pull:
	cd docker && docker compose pull

clean:
	cd docker && docker compose down -v --remove-orphans

run:
	cd docker && docker compose up

run-local:
	make build-local && cd docker && docker compose up

run-geth:
	cd docker && COMPOSE_PROFILES=op-geth docker compose up

run-erigon:
	cd docker && COMPOSE_PROFILES=op-erigon docker compose up
