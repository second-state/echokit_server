# echokit-server + Silero VAD Docker Image

This directory provides a single-stage runtime image that launches both `echokit_server` and `silero_vad_server` inside the same container.

- **Runtime image** (`debian:bookworm-slim`): installs runtime dependencies, downloads the CUDA-enabled `libtorch` bundle, and fetches the `v0.1.0` release binaries for `echokit_server`, `silero_vad_server`, and the `silero_vad.jit` model.
- **Supervisor script**: `/usr/local/bin/start_servers.sh` starts both services, relays signals, and keeps the container alive while either process is running.

## Run

Expose the application ports and mount your configuration plus a writable recordings directory:

```sh
docker run --rm \
  -p 8080:8080 \
  -v $(pwd)/config.toml:/app/config.toml \
  -v $(pwd)/record:/app/record \
  secondstate/echokit:latest-server-vad
```

Mount your `config.toml` directly into `/app/config.toml`. If you need to override additional assets such as `silero_vad.jit`, mount each file individually alongside the config. The servers write generated artifacts to `/app/record`, so ensure the local `record` directory exists and is writable. The container sets `RUST_LOG=info` and runs `start_servers.sh` by default, keeping both services available on ports `8080` and `8000` inside the container.

The VAD server listens on port `8000` internally. Choose one of the following so `echokit_server` talks to it correctly without publishing the VAD port to the host:

1. Update your mounted `config.toml` so `vad_url` and `vad_realtime_url` point to `http://localhost:8000` / `ws://localhost:8000`.
2. Keep the default config (`9093`) and add `-e VAD_LISTEN=9093` to the `docker run` command so the VAD server binds that port inside the container.

## Build

```sh
docker build \
  --platform linux/amd64 \
  --build-arg ECHOKIT_VERSION=0.1.0 \
  -t secondstate/echokit:latest-server-vad \
  .
```

This Dockerfile always downloads the `linux-x86_64` release artifacts baked into the file. Update the URLs if you need to pin a different release.

## Platform support

Multi-platform builds are **not supported**. The image bundles CUDA-enabled `libtorch` and hard-coded `x86_64-unknown-linux-gnu` binaries for both servers, so `docker buildx` cannot produce working arm64 (or other architecture) variants.

## Publish

```sh
docker login
docker push secondstate/echokit:latest-server-vad
```
