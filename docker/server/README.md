# echokit-server Standalone Docker Image

This directory contains a multi-stage Dockerfile for producing a lean runtime image of `echokit_server`.

- **Builder stage** (`rust:1.85-slim`): installs the minimal Rust toolchain dependencies, clones `https://github.com/second-state/echokit_server`, and compiles the project in release mode.
- **Runtime stage** (`debian:bookworm-slim`): installs the required runtime libraries, copies the compiled binary and default `config.toml`, and sets `RUST_LOG=info` before starting the server with that config.

## Run

Mount your local `config.toml` so the container uses your configuration:

```sh
docker run --rm -p 8080:8080 -v $(pwd)/config.toml:/app/config.toml secondstate/echokit:latest-server
```

The container executes `echokit_server config.toml` by default, reading logs at the `info` level.

## Build

```sh
docker build -t secondstate/echokit:latest-server .
```

## Multi-platform build

```sh
docker buildx build . --platform linux/arm64,linux/amd64 --tag secondstate/echokit:latest-server -f Dockerfile
```

## Publish 

```sh
docker login
docker push secondstate/echokit:latest-server
```

