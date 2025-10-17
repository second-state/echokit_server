# syntax=docker/dockerfile:1.6

FROM debian:bookworm-slim

WORKDIR /app

ARG ECHOKIT_VERSION=0.1.0
ARG VAD_VERSION=0.1.0

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        bash \
        ca-certificates \
        curl \
        unzip \
        libgomp1 \
        libssl3 \
    && rm -rf /var/lib/apt/lists/*

RUN set -eux; \
    curl -L https://download.pytorch.org/libtorch/cu124/libtorch-cxx11-abi-shared-with-deps-2.4.0%2Bcu124.zip -o /tmp/libtorch.zip; \
    unzip /tmp/libtorch.zip -d /usr/local/lib; \
    rm /tmp/libtorch.zip; \
    if [ -f /usr/local/lib/libtorch/lib/libnvrtc-builtins.so ]; then \
        ln -sf /usr/local/lib/libtorch/lib/libnvrtc-builtins.so /usr/local/lib/libtorch/lib/libnvrtc-builtins.so.12.4; \
    fi

RUN set -eux; \
    ARCH_SUFFIX="x86_64-unknown-linux-gnu"; \
    tmpdir="$(mktemp -d)"; \
    archive="echokit_server-v${ECHOKIT_VERSION}-${ARCH_SUFFIX}.tar.gz"; \
    url="https://github.com/second-state/echokit_server/releases/download/v${ECHOKIT_VERSION}/${archive}"; \
    curl -fsSL -o /tmp/echokit_server.tar.gz "${url}"; \
    tar -xzf /tmp/echokit_server.tar.gz -C "$tmpdir"; \
    bin_path="$(find "$tmpdir" -type f -name 'echokit_server' -print -quit)"; \
    test -n "$bin_path"; \
    install -m 0755 "$bin_path" /usr/local/bin/echokit_server; \
    rm -rf "$tmpdir" /tmp/echokit_server.tar.gz

RUN set -eux; \
    ARCH_SUFFIX="x86_64-unknown-linux-gnu"; \
    tmpdir="$(mktemp -d)"; \
    archive="silero_vad_server-v${VAD_VERSION}-${ARCH_SUFFIX}.tar.gz"; \
    url="https://github.com/second-state/silero_vad_server/releases/download/v${VAD_VERSION}/${archive}"; \
    curl -fsSL -o /tmp/silero_vad_server.tar.gz "${url}"; \
    tar -xzf /tmp/silero_vad_server.tar.gz -C "$tmpdir"; \
    vad_bin="$(find "$tmpdir" -type f -name 'silero_vad_server' -print -quit)"; \
    test -n "$vad_bin"; \
    install -m 0755 "$vad_bin" /usr/local/bin/silero_vad_server; \
    rm -rf "$tmpdir" /tmp/silero_vad_server.tar.gz

RUN curl -fsSL "https://github.com/second-state/silero_vad_server/raw/refs/tags/v${VAD_VERSION}/silero_vad.jit" -o /app/silero_vad.jit

ENV LD_LIBRARY_PATH=/usr/local/lib/libtorch/lib:$LD_LIBRARY_PATH
ENV LIBTORCH=/usr/local/lib/libtorch

COPY config.toml .
COPY hello.wav .

RUN cat <<'EOF' > /usr/local/bin/start_servers.sh \
    && chmod +x /usr/local/bin/start_servers.sh
#!/usr/bin/env bash
set -euo pipefail

vad_pid=""
server_pid=""

terminate() {
    if [[ -n "$vad_pid" ]]; then
        kill -TERM "$vad_pid" 2>/dev/null || true
    fi
    if [[ -n "$server_pid" ]]; then
        kill -TERM "$server_pid" 2>/dev/null || true
    fi
}

silero_vad_server &
vad_pid=$!

echokit_server config.toml &
server_pid=$!

trap terminate SIGINT SIGTERM EXIT

wait -n "$vad_pid" "$server_pid" || true
terminate
wait || true
EOF

ENV RUST_LOG=info

CMD ["/usr/local/bin/start_servers.sh"]
