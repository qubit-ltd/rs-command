#!/usr/bin/env bash
set -euo pipefail

project_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)
"$project_root/.infra/bootstrap.sh" .infra/bin/prepare-local-path-dependencies.sh "$@"
cargo fetch --locked --manifest-path "$project_root/Cargo.toml"
