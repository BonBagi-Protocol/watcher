# Changelog

## 0.1.0 — 2026-10-27

First release, for ZECATHON 2026.

- `check`: verifies the charter (every member's ZIP 215 Ed25519 signature over the canonical
  bytes of `@bonbagi/core`), finds the member's outputs by memo fingerprint, checks member slot,
  pool, amount (exact with `--price` or a pay link's price, otherwise "a share of some total")
  and confirmations, and reports PAID, PENDING, MISMATCH, EXPIRED or NOT FOUND. Text or `--json`;
  exit 0/1/2.
- Sources: `--snapshot` (offline) and a live scan with the member's own unified full viewing key
  through a watch-only `zcash-devtool` wallet (`b532d4c`), with `--server`, `--connection tor`
  and `--save-snapshot`. The viewing key is read from a file or `BONBAGI_UFVK`, never from the
  command line.
- Charter from a file or straight from a pay link's `#c=` fragment.
- Receipts: `check --receipt` signs "my share of order X arrived" with the member's charter key;
  `verify-receipt` checks one offline. Verifiable in TypeScript with `@bonbagi/core`.
- Conformance vectors generated from `@bonbagi/core` 0.1.0 (`cdb7e0f`); a constructed testnet
  example with golden outputs.
- Dockerfile with `zcash-devtool`; CI: fmt, clippy, tests, golden outputs, gitleaks; image
  workflow: golden outputs inside the image, live testnet scan of a fresh empty viewing key,
  publish to `ghcr.io/bonbagi/watcher`.
