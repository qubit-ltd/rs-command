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
# Code coverage testing script
# Uses cargo-llvm-cov to generate code coverage reports
#

set -euo pipefail

echo "🔍 Starting code coverage testing..."

MIN_FUNCTION_PERCENT=100
MIN_LINE_PERCENT=98
MIN_REGION_PERCENT=98
COVERAGE_THRESHOLDS=(
    --fail-under-functions "$MIN_FUNCTION_PERCENT"
    --fail-under-lines "$MIN_LINE_PERCENT"
    --fail-under-regions "$MIN_REGION_PERCENT"
)

# Switch to project directory
cd "$(dirname "$0")"

# Detect package name from Cargo.toml
if [ -f "Cargo.toml" ]; then
    PACKAGE_NAME=$(grep "^name = " Cargo.toml | head -n 1 | sed 's/name = "\(.*\)"/\1/')
    echo "📦 Detected package: $PACKAGE_NAME"
else
    echo "❌ Error: Cargo.toml not found in current directory"
    exit 1
fi

# Get current directory absolute path to filter coverage
CURRENT_CRATE_DIR=$(pwd)
echo "📁 Coverage will only include files in: $CURRENT_CRATE_DIR"
echo "🎯 Coverage thresholds: functions 100%, lines >98%, regions >98% per source file"

# Build regex pattern to exclude third-party code and other workspace members
CURRENT_CRATE_NAME=$(basename "$CURRENT_CRATE_DIR")
WORKSPACE_ROOT=$(cd "$(dirname "$0")/.." && pwd)

# Create list of other workspace crates to exclude (any sibling directory)
OTHER_CRATES=""
for crate_dir in "$WORKSPACE_ROOT"/*/; do
    [ -d "$crate_dir" ] || continue
    crate_name=$(basename "$crate_dir")
    if [ "$crate_name" != "$CURRENT_CRATE_NAME" ]; then
        if [ -z "$OTHER_CRATES" ]; then
            OTHER_CRATES="$crate_name"
        else
            OTHER_CRATES="$OTHER_CRATES|$crate_name"
        fi
    fi
done

# Exclude: cargo registry, rustup, and other workspace crates
# Using simple alternation for clarity
EXCLUDE_PATTERN="(\.cargo/registry|\.rustup/|/($OTHER_CRATES)/)"
echo "🚫 Excluding: .cargo/registry, .rustup, and other workspace members"

check_json_source_file_thresholds() {
    local json_path="$1"

    if ! command -v jq > /dev/null; then
        echo "❌ Error: jq is required to validate per-source JSON coverage thresholds"
        exit 1
    fi

    local failures
    failures=$(jq -r \
        --arg src_dir "$CURRENT_CRATE_DIR/src/" \
        --argjson min_functions "$MIN_FUNCTION_PERCENT" \
        --argjson min_lines "$MIN_LINE_PERCENT" \
        --argjson min_regions "$MIN_REGION_PERCENT" \
        '
        .data[].files[]
        | select(.filename | startswith($src_dir))
        | .filename as $file
        | .summary as $summary
        | select(
            ($summary.functions.percent < $min_functions)
            or ($summary.lines.percent <= $min_lines)
            or ($summary.regions.percent <= $min_regions)
        )
        | "\($file): functions \($summary.functions.percent)% (\($summary.functions.covered)/\($summary.functions.count)), lines \($summary.lines.percent)% (\($summary.lines.covered)/\($summary.lines.count)), regions \($summary.regions.percent)% (\($summary.regions.covered)/\($summary.regions.count))"
        ' "$json_path")

    if [ -n "$failures" ]; then
        echo "❌ Per-source coverage thresholds failed:"
        echo "$failures"
        echo "   Required: functions 100%, lines >98%, regions >98%"
        exit 1
    fi

    echo "✅ Per-source JSON coverage thresholds passed"
}

# Parse arguments, check if cleanup is needed
CLEAN_FLAG=""
FORMAT_ARG=""

for arg in "$@"; do
    case "$arg" in
        --clean)
            CLEAN_FLAG="yes"
            ;;
        *)
            FORMAT_ARG="$arg"
            ;;
    esac
done

# Default format is html
FORMAT_ARG="${FORMAT_ARG:-html}"

# Ensure explicit output paths used by cargo-llvm-cov exist.
mkdir -p target/llvm-cov

# If --clean option is specified, clean old data
if [ "$CLEAN_FLAG" = "yes" ]; then
    echo "🧹 Cleaning old coverage data..."
    cargo llvm-cov clean
else
    echo "ℹ️  Using cached build (use --clean option if you need to clean cache)"
fi

# cargo-llvm-cov does not create parent directories for --json/--lcov/--cobertura outputs
mkdir -p target/llvm-cov

# Run tests and generate coverage reports
case "$FORMAT_ARG" in
    html)
        echo "📊 Generating HTML format coverage report..."
        cargo llvm-cov --package "$PACKAGE_NAME" --html --open \
            "${COVERAGE_THRESHOLDS[@]}" \
            --ignore-filename-regex "$EXCLUDE_PATTERN"
        echo "✅ HTML report generated and opened in browser"
        echo "   Report location: target/llvm-cov/html/index.html"
        ;;

    text)
        echo "📊 Generating text format coverage report..."
        cargo llvm-cov --package "$PACKAGE_NAME" \
            "${COVERAGE_THRESHOLDS[@]}" \
            --ignore-filename-regex "$EXCLUDE_PATTERN"
        ;;

    lcov)
        echo "📊 Generating LCOV format coverage report..."
        cargo llvm-cov --package "$PACKAGE_NAME" --lcov --output-path target/llvm-cov/lcov.info \
            "${COVERAGE_THRESHOLDS[@]}" \
            --ignore-filename-regex "$EXCLUDE_PATTERN"
        echo "✅ LCOV report generated"
        echo "   Report location: target/llvm-cov/lcov.info"
        ;;

    json)
        echo "📊 Generating JSON format coverage report..."
        cargo llvm-cov --package "$PACKAGE_NAME" --json --output-path target/llvm-cov/coverage.json \
            "${COVERAGE_THRESHOLDS[@]}" \
            --ignore-filename-regex "$EXCLUDE_PATTERN"
        check_json_source_file_thresholds target/llvm-cov/coverage.json
        echo "✅ JSON report generated"
        echo "   Report location: target/llvm-cov/coverage.json"
        ;;

    cobertura)
        echo "📊 Generating Cobertura XML format coverage report..."
        cargo llvm-cov --package "$PACKAGE_NAME" --cobertura --output-path target/llvm-cov/cobertura.xml \
            "${COVERAGE_THRESHOLDS[@]}" \
            --ignore-filename-regex "$EXCLUDE_PATTERN"
        echo "✅ Cobertura report generated"
        echo "   Report location: target/llvm-cov/cobertura.xml"
        ;;

    all)
        echo "📊 Generating all format coverage reports..."

        # HTML
        echo "  - Generating HTML report..."
        cargo llvm-cov --package "$PACKAGE_NAME" --html \
            "${COVERAGE_THRESHOLDS[@]}" \
            --ignore-filename-regex "$EXCLUDE_PATTERN"

        # LCOV
        echo "  - Generating LCOV report..."
        cargo llvm-cov --package "$PACKAGE_NAME" --lcov --output-path target/llvm-cov/lcov.info \
            "${COVERAGE_THRESHOLDS[@]}" \
            --ignore-filename-regex "$EXCLUDE_PATTERN"

        # JSON
        echo "  - Generating JSON report..."
        cargo llvm-cov --package "$PACKAGE_NAME" --json --output-path target/llvm-cov/coverage.json \
            "${COVERAGE_THRESHOLDS[@]}" \
            --ignore-filename-regex "$EXCLUDE_PATTERN"
        check_json_source_file_thresholds target/llvm-cov/coverage.json

        # Cobertura
        echo "  - Generating Cobertura XML report..."
        cargo llvm-cov --package "$PACKAGE_NAME" --cobertura --output-path target/llvm-cov/cobertura.xml \
            "${COVERAGE_THRESHOLDS[@]}" \
            --ignore-filename-regex "$EXCLUDE_PATTERN"

        echo "✅ All format reports generated"
        echo "   HTML:      target/llvm-cov/html/index.html"
        echo "   LCOV:      target/llvm-cov/lcov.info"
        echo "   JSON:      target/llvm-cov/coverage.json"
        echo "   Cobertura: target/llvm-cov/cobertura.xml"
        ;;

    help|--help|-h)
        echo "Usage: ./coverage.sh [format] [options]"
        echo ""
        echo "Format options:"
        echo "  html       Generate HTML report and open in browser (default)"
        echo "  text       Output text format report to terminal"
        echo "  lcov       Generate LCOV format report"
        echo "  json       Generate JSON format report"
        echo "  cobertura  Generate Cobertura XML format report"
        echo "  all        Generate all format reports"
        echo "  help       Show this help information"
        echo ""
        echo "Options:"
        echo "  --clean    Clean old coverage data and build cache before running"
        echo "             By default, cached builds are used to speed up compilation"
        echo ""
        echo "Thresholds:"
        echo "  functions 100%, lines >98%, regions >98% for every source file in src/"
        echo ""
        echo "Performance tips:"
        echo "  • First run will be slower (needs to compile all dependencies)"
        echo "  • Subsequent runs will be much faster (using cache)"
        echo "  • Only use --clean when dependencies are updated or major code changes"
        echo ""
        echo "Examples:"
        echo "  ./coverage.sh              # Generate HTML report (using cache)"
        echo "  ./coverage.sh text         # Output text report (using cache)"
        echo "  ./coverage.sh --clean      # Clean then generate HTML report"
        echo "  ./coverage.sh html --clean # Clean then generate HTML report"
        echo "  ./coverage.sh all --clean  # Clean then generate all formats"
        exit 0
        ;;

    *)
        echo "❌ Error: Unknown format '$1'"
        echo "Run './coverage.sh help' to see available options"
        exit 1
        ;;
esac

echo "✅ Code coverage testing completed!"
