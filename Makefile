.PHONY: build-all build-local

build-all:
	docker buildx build --platform linux/arm64,linux/amd64 -t noah7545/magi .

build-local:
	docker buildx build -t noah7545/magi .
