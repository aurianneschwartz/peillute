FROM rustlang/rust:nightly AS builder

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libgtk-3-dev \
    libsqlite3-0 \
    libwebkit2gtk-4.1-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    git \
    curl \
    build-essential \
 && rm -rf /var/lib/apt/lists/*

RUN rustup target add wasm32-unknown-unknown

WORKDIR /app
COPY . .

# Install dioxus-cli: try prebuilt via cargo-binstall; if not available, fall back to locked git install
RUN set -eux; \
  curl -L --proto '=https' --tlsv1.2 -sSf \
    https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh \
    | bash; \
  export CARGO_BINSTALL_NO_COMPILE=1; \
  if ! cargo binstall -y dioxus-cli@0.6.3; then \
    cargo install --git https://github.com/DioxusLabs/dioxus --tag v0.6.3 dioxus-cli --locked; \
  fi

RUN RUST_LOG=debug RUST_BACKTRACE=1 dx bundle --release --platform web

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      ca-certificates \
      libsqlite3-0 \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/dx/peillute/release/web /app/web

RUN printf '%s\n' \
  '#!/bin/sh' \
  'set -e' \
  'ARGS="$@"' \
  'if [ -n "$CLI_IP" ]; then' \
  '  ARGS="$ARGS --cli-ip $CLI_IP"' \
  'fi' \
  'if [ -n "$CLI_PORT" ]; then' \
  '  ARGS="$ARGS --cli-port $CLI_PORT"' \
  'fi' \
  'if [ -n "$CLI_PEERS" ]; then' \
  '  ARGS="$ARGS --cli-peers $CLI_PEERS"' \
  'fi' \
  'if [ -n "$JAEGER_ADR" ]; then' \
  '  ARGS="$ARGS --jaeger-adr $JAEGER_ADR"' \
  'fi' \
  'if [ -n "$LOKI_ADR" ]; then' \
  '  ARGS="$ARGS --loki-adr $LOKI_ADR"' \
  'fi' \
  'export RUST_LOG=debug && exec /app/web/server $ARGS' \
  > /usr/local/bin/entrypoint.sh \
 && chmod +x /usr/local/bin/entrypoint.sh

RUN useradd -m -u 1000 appuser && chown -R appuser:appuser /app
USER appuser

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
CMD []