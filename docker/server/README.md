# echokit-server Standalone Docker Image

This directory contains a single-stage Dockerfile that produces a lean runtime image of `echokit_server` by downloading official release binaries.

- **Runtime image** (`debian:bookworm-slim`): installs the required runtime libraries, fetches the architecture-specific tarball from GitHub releases (currently `v0.1.0`), places the `echokit_server` binary in `/usr/local/bin`, copies the default `config.toml`, and sets `RUST_LOG=info` before starting the server with that config.

## Run

Mount your local configuration directory and a writable recordings directory:

```sh
docker run --rm \
  -p 8080:8080 \
  -v $(pwd)/config.toml:/app/config.toml \
  -v $(pwd)/record:/app/record \
  secondstate/echokit:latest-server
```

Mount your `config.toml` directly into `/app/config.toml`. If you need to override additional assets such as `hello.wav`, mount them individually alongside the config file. The server writes any generated artifacts to `/app/record`, so ensure the `record` directory exists locally and is writable. The container executes `echokit_server config.toml` by default, reading logs at the `info` level.

## Build

```sh
docker build -t secondstate/echokit:latest-server -f docker/server/Dockerfile .
```

The build automatically selects the correct release artifact for your build architecture using the BuildKit-provided `TARGETPLATFORM`/`TARGETARCH` arguments. Override the downloaded release by supplying `--build-arg ECHOKIT_VERSION=<version>` (for example `0.1.1`) if you want a different tag.

## Multi-platform build

Use Buildx to produce and publish a multi-arch manifest in one command. BuildKit injects `TARGETPLATFORM` (`linux/amd64`, `linux/arm64`, etc.), so you do not need to set them manually.

```sh
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  --push \
  --build-arg ECHOKIT_VERSION=0.1.0 \
  --tag secondstate/echokit:latest-server \
  -f docker/server/Dockerfile .
```

Adjust `ECHOKIT_VERSION` as needed; omit the flag to fall back to the default version baked into the Dockerfile.

## Publish

```sh
docker login
docker push secondstate/echokit:latest-server
```
