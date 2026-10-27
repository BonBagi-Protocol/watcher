# bonbagi-watcher plus zcash-devtool for live scans. The live scan creates a watch-only wallet
# from your viewing key: it can see what arrived, it can never spend.
#   docker build -t bonbagi-watcher .
#   docker run --rm bonbagi-watcher check --charter examples/testnet/charter.json \
#     --snapshot examples/testnet/snapshot.json --member 1

FROM rust:1.99.1-slim-bookworm AS devtool
RUN apt-get update \
 && apt-get install -y --no-install-recommends build-essential pkg-config git ca-certificates \
      libsqlite3-dev libssl-dev \
 && rm -rf /var/lib/apt/lists/*
# The watcher parses devtool's text output; this is the commit it is written and tested against.
ARG ZCASH_DEVTOOL_REV=b532d4cd760d30213c961fe0638ee6b6e1bf8e30
RUN cargo install --locked --git https://github.com/zcash/zcash-devtool --rev "${ZCASH_DEVTOOL_REV}" zcash-devtool

FROM rust:1.99.1-slim-bookworm AS watcher
WORKDIR /src
COPY . .
# The image already has 1.99.1; skip the components rust-toolchain.toml asks for (fmt, clippy).
ENV RUSTUP_TOOLCHAIN=1.99.1
RUN cargo install --locked --path . --root /out

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates libsqlite3-0 libssl3 \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --create-home --uid 10001 watcher \
 && mkdir -p /work /data \
 && chown watcher:watcher /work /data
COPY --from=devtool /usr/local/cargo/bin/zcash-devtool /usr/local/bin/zcash-devtool
COPY --from=watcher /out/bin/bonbagi-watcher /usr/local/bin/bonbagi-watcher
COPY --chmod=755 scripts/live-smoke.sh /usr/local/bin/live-smoke
COPY --chown=watcher:watcher examples /work/examples
LABEL org.opencontainers.image.source="https://github.com/BonBagi/watcher" \
      org.opencontainers.image.description="BonBagi watcher: check your own share of a BonBagi payment with your own viewing key" \
      org.opencontainers.image.licenses="MIT"
WORKDIR /work
USER watcher
ENTRYPOINT ["bonbagi-watcher"]
CMD ["--help"]
