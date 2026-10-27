#!/usr/bin/env bash
# Every check CI runs, in one local command. Exits non-zero on the first failure.
#   bash scripts/check.sh
set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf '\n== %s\n' "$*"; }

step "cargo fmt --check";   cargo fmt --all -- --check
step "cargo clippy";        cargo clippy --all-targets --locked -- -D warnings
step "cargo test";          cargo test --locked

step "secret scan"
if command -v gitleaks >/dev/null 2>&1; then
  gitleaks git --config .gitleaks.toml --redact --no-banner --verbose --exit-code 1 .
else
  echo "gitleaks not installed locally; CI runs it on every push (https://github.com/gitleaks/gitleaks/releases)."
fi

printf '\nAll checks passed.\n'
