.PHONY: build-all build-local run run-local run-geth

build-all:
	docker buildx build --platform linux/arm64,linux/amd64 -t noah7545/magi --push .

build-local:
	docker buildx build -t noah7545/magi --load .

run:
	cd docker && docker compose pull && docker compose up

run-local:
	make build-local && cd docker && docker compose up

run-geth:
	cd docker && COMPOSE_PROFILES=no-magi docker compose up

