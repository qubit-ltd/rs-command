#!/usr/bin/env bash
set -euo pipefail

project_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
cd "$project_root"

cargo bench --bench command_runner -- silent_command_runner --test
