# syntax=docker/dockerfile:1.6

FROM rust:1.85-slim AS builder

WORKDIR /app

# Install build dependencies required by reqwest/rustls toolchain.
RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev build-essential git \
    && rm -rf /var/lib/apt/lists/*

RUN git clone https://github.com/second-state/echokit_server.git
RUN cd echokit_server \
    && cargo build --release


FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/echokit_server/config.toml .
COPY --from=builder /app/echokit_server/target/release/echokit_server /usr/local/bin/echokit_server

ENV RUST_LOG=info

CMD ["echokit_server", "config.toml"]
