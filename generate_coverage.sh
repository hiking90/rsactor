#!/bin/bash

set -e

# Function to check if a command exists
command_exists() {
    type "$1" &> /dev/null
}

# Check if cargo-llvm-cov is installed
if ! command_exists cargo-llvm-cov; then
    echo "cargo-llvm-cov is not installed."
    read -p "Do you want to install cargo-llvm-cov? (y/N): " choice
    case "$choice" in
        y|Y)
            echo "Installing cargo-llvm-cov..."
            cargo install cargo-llvm-cov
            if ! command_exists cargo-llvm-cov; then
                echo "Failed to install cargo-llvm-cov. Please install it manually and try again."
                exit 1
            fi
            ;;
        *)
            echo "cargo-llvm-cov is required to generate coverage reports. Please install it and try again."
            echo "You can install it with: cargo install cargo-llvm-cov"
            echo "Or with rustup: rustup component add llvm-tools-preview"
            exit 1
            ;;
    esac
fi

# Check if llvm-tools is installed (component name varies: llvm-tools-preview or llvm-tools)
if ! rustup component list | grep -q "llvm-tools.*installed"; then
    echo "llvm-tools component is not installed."
    read -p "Do you want to install it via rustup? (y/N): " choice
    case "$choice" in
        y|Y)
            echo "Installing llvm-tools..."
            rustup component add llvm-tools
            ;;
        *)
            echo "llvm-tools is required for coverage. Please install it and try again."
            echo "You can install it with: rustup component add llvm-tools"
            exit 1
            ;;
    esac
fi

# Create output directory
OUTPUT_DIR="target/coverage"
mkdir -p "$OUTPUT_DIR"

echo "========================================"
echo "Running tests with coverage collection"
echo "========================================"

# Clean previous coverage data
echo "Cleaning previous coverage data..."
cargo llvm-cov clean --workspace

# Run all tests with all features enabled to ensure complete coverage
# --all-features includes 'test-utils' which is required for debugging_tools_tests
echo ""
echo "Running tests with --all-features..."
cargo llvm-cov --all-features --workspace \
    --ignore-filename-regex '(tests/|examples/|rsactor-derive/)' \
    --html --output-dir "$OUTPUT_DIR/report"

# Also generate a text summary
echo ""
echo "========================================"
echo "Coverage Summary"
echo "========================================"
cargo llvm-cov report --ignore-filename-regex '(tests/|examples/|rsactor-derive/)'

# Generate lcov format for CI integration (optional)
cargo llvm-cov report --ignore-filename-regex '(tests/|examples/|rsactor-derive/)' \
    --lcov --output-path "$OUTPUT_DIR/lcov.info" 2>/dev/null || true

echo ""
echo "========================================"
echo "Coverage report generated!"
echo "========================================"
echo "HTML report: $OUTPUT_DIR/report/html/index.html"
echo "LCOV format: $OUTPUT_DIR/lcov.info"
echo ""
echo "Open the HTML report with:"
echo "  open $OUTPUT_DIR/report/html/index.html"
echo ""
echo "Done."
