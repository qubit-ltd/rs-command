#!/bin/bash

################################################################################
#
#    Copyright (c) 2026.
#    Haixing Hu, Qubit Co. Ltd.
#
#    All rights reserved.
#
################################################################################
# Format script using nightly rustfmt
# This script uses nightly rustfmt for advanced formatting features

set -euo pipefail

# Switch to project directory
cd "$(dirname "$0")"

# Check if nightly toolchain is installed
if ! rustup toolchain list | grep -q nightly; then
    echo "Installing nightly toolchain..."
    rustup toolchain install nightly
fi

# Run cargo fmt with nightly toolchain
echo "Running cargo +nightly fmt..."
cargo +nightly fmt

echo "Formatting complete!"
