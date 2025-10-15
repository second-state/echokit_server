# echokit-server Standalone Docker Image

This directory contains a multi-stage Dockerfile for producing a lean runtime image of `echokit_server`.

- **Builder stage** (`rust:1.85-slim`): installs the minimal Rust toolchain dependencies, copies the repository contents into `/app`, and compiles the project in release mode.
- **Runtime stage** (`debian:bookworm-slim`): installs the required runtime libraries, copies the compiled binary and default `config.toml`, and sets `RUST_LOG=info` before starting the server with that config.

## Run

Mount your local configuration directory and a writable recordings directory:

```sh
docker run --rm \
  -p 8080:8080 \
  -v $(pwd)/config:/app \
  -v $(pwd)/record:/app/record \
  secondstate/echokit:latest-server
```

Place `config.toml` and `hello.wav` inside the mounted `config` directory so they appear in `/app` inside the container. The server writes any generated artifacts to `/app/record`, so ensure the `record` directory exists locally and is writable. The container executes `echokit_server config.toml` by default, reading logs at the `info` level.

## Build

```sh
docker build -t secondstate/echokit:latest-server -f docker/server/Dockerfile .
```

## Multi-platform build

```sh
docker buildx build . --platform linux/arm64,linux/amd64 --tag secondstate/echokit:latest-server -f docker/server/Dockerfile
```

## Publish 

```sh
docker login
docker push secondstate/echokit:latest-server
```
