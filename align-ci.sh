#!/bin/bash
################################################################################
#
#    Copyright (c) 2026.
#    Haixing Hu, Qubit Co. Ltd.
#
#    All rights reserved.
#
################################################################################
#
# One-shot auto-fix to match local CI (fmt + clippy on all targets, then verify).
# Run from repo root: ./align-ci.sh
#

set -euo pipefail

cd "$(dirname "$0")"

if ! rustup toolchain list | grep -q nightly; then
    echo "Installing nightly toolchain..."
    rustup toolchain install nightly
fi

echo "==> cargo +nightly fmt"
cargo +nightly fmt

echo "==> cargo +nightly clippy --fix (all targets / features)"
cargo +nightly clippy --fix --allow-dirty --allow-staged --all-targets --all-features

echo "==> cargo +nightly clippy (verify, -D warnings)"
cargo +nightly clippy --all-targets --all-features -- -D warnings

echo "==> RUSTFLAGS=--cfg coverage cargo +nightly clippy (coverage-only code)"
RUSTFLAGS="--cfg coverage" cargo +nightly clippy --all-targets --all-features -- -D warnings

if command -v cargo-llvm-cov > /dev/null && command -v jq > /dev/null; then
    echo "==> ./coverage.sh json"
    ./coverage.sh json
else
    echo "Skipping ./coverage.sh json because cargo-llvm-cov or jq is not installed."
fi

echo "Done. CI-style checks should pass; run ./ci-check.sh for the full pipeline."
