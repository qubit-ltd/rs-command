#!/usr/bin/env bash
set -euo pipefail

project_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
cd "$project_root"

if [[ "${RUNNER_OS:-}" == "Windows" ]]; then
    cargo test --lib windows_ -- --nocapture
fi

package_files=$(cargo package --list --allow-dirty)
for example_file in managed_collect managed_session managed_file managed_shutdown; do
    found_example=false
    while IFS= read -r package_file; do
        if [[ "$package_file" == "examples/${example_file}.rs" ]]; then
            found_example=true
            break
        fi
    done <<<"$package_files"
    if [[ "$found_example" != true ]]; then
        echo "missing packaged example: ${example_file}" >&2
        exit 1
    fi
done

cargo test --all-targets --no-default-features
cargo test --all-targets --features async
cargo test --doc --no-default-features
cargo test --doc --features async
cargo test --test documentation_examples_tests
cargo test --examples --features async
cargo test --features async --test managed_handoff_tests

cargo bench --bench command_runner -- silent_command_runner --test
