#!/usr/bin/env bash
set -euo pipefail

project_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
cd "$project_root"

if [[ "${RUNNER_OS:-}" == "Windows" ]]; then
    cargo test --lib windows_ -- --nocapture
fi

cargo test --all-targets --no-default-features
cargo test --all-targets --features async
cargo test --doc --no-default-features
cargo test --doc --features async
cargo test --test documentation_examples_tests
cargo test --features async --test managed_documentation_tests
cargo test --features async --test managed_handoff_tests

cargo bench --bench command_runner -- silent_command_runner --test
