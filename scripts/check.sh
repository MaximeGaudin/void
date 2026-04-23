#!/usr/bin/env bash
# Pre-flight checks that mirror the GitHub Actions CI workflow exactly.
# Run this before pushing to catch CI failures locally.
#
# Usage:
#   ./scripts/check.sh           # run all checks
#   ./scripts/check.sh --no-test # skip the (slow) test step
set -euo pipefail

# Match CI: promote all rustc warnings to errors.
export RUSTFLAGS="${RUSTFLAGS:--D warnings}"

RUN_TESTS=1
for arg in "$@"; do
  case "$arg" in
    --no-test) RUN_TESTS=0 ;;
    -h|--help)
      sed -n '2,9p' "$0"
      exit 0
      ;;
    *)
      echo "Unknown argument: $arg" >&2
      exit 2
      ;;
  esac
done

echo "==> cargo fmt --check"
cargo fmt --check

echo "==> cargo clippy --all-targets -- -D warnings"
cargo clippy --all-targets -- -D warnings

if [ "$RUN_TESTS" -eq 1 ]; then
  echo "==> cargo test"
  cargo test
else
  echo "==> cargo test (skipped via --no-test)"
fi

echo "==> All pre-flight checks passed"
