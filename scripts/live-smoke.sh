#!/bin/sh
# Live-scan smoke test, run inside the Docker image (CI does: docker run --entrypoint live-smoke).
# A brand-new testnet wallet is created only to get a fresh viewing key; its seed is deleted, and
# the watcher scans that viewing key from the wallet's birthday through a real lightwalletd.
# Nothing was ever paid to it, so the right answer is NOT FOUND (exit 1). This proves the whole
# live path (init-fvk, sync, enhance, list-tx, balance) end to end without anybody's funds.
set -eu
work=$(mktemp -d)
cd "$work"
echo | zcash-devtool wallet -w seed init --name smoke --identity id.txt --network test >/dev/null 2>&1
accounts=$(zcash-devtool wallet -w seed list-accounts)
birthday=$(printf '%s\n' "$accounts" | sed -n 's/.*(birthday height \([0-9]*\)).*/\1/p' | head -n 1)
printf '%s\n' "$accounts" | sed -n 's/^ *UFVK: //p' | head -n 1 > ufvk.txt
rm -rf seed id.txt
test -n "$birthday" && grep -q '^uviewtest1' ufvk.txt

set +e
bonbagi-watcher check --charter /work/examples/testnet/charter.json --member 1 \
  --ufvk-file ufvk.txt --birthday "$birthday" --order ord-k7m2qx9p "$@" > out.txt
code=$?
set -e
cat out.txt
test "$code" = 1
grep -q 'Source   live scan, testnet' out.txt
grep -q '=> NOT FOUND' out.txt
echo "live smoke: OK (exit $code; fresh testnet viewing key scanned from block $birthday)"
