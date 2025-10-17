# syntax=docker/dockerfile:1.6

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3 curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

ARG TARGETPLATFORM
ARG ECHOKIT_VERSION=0.1.0

RUN set -eux; \
    case "${TARGETPLATFORM}" in \
    "linux/amd64") ARCH_SUFFIX="x86_64-unknown-linux-gnu" ;; \
    "linux/arm64") ARCH_SUFFIX="aarch64-unknown-linux-gnu" ;; \
    *) echo "Unsupported TARGETPLATFORM: ${TARGETPLATFORM}" >&2; exit 1 ;; \
    esac; \
    ARCHIVE="echokit_server-v${ECHOKIT_VERSION}-${ARCH_SUFFIX}.tar.gz"; \
    URL="https://github.com/second-state/echokit_server/releases/download/v${ECHOKIT_VERSION}/${ARCHIVE}"; \
    tmpdir="$(mktemp -d)"; \
    curl -fsSL -o /tmp/echokit_server.tar.gz "${URL}"; \
    tar -xzf /tmp/echokit_server.tar.gz -C "$tmpdir"; \
    bin_path="$(find "$tmpdir" -type f -name 'echokit_server' -print -quit)"; \
    test -n "$bin_path"; \
    install -m 0755 "$bin_path" /usr/local/bin/echokit_server; \
    rm -rf "$tmpdir" /tmp/echokit_server.tar.gz

COPY config.toml .
COPY hello.wav .

ENV RUST_LOG=info

CMD ["echokit_server", "config.toml"]
