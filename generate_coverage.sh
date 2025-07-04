#!/bin/bash

# Function to check if a command exists
command_exists () {
    type "$1" &> /dev/null ;
}

# Check if grcov is installed
if ! command_exists grcov ; then
    echo "grcov is not installed."
    read -p "Do you want to install grcov? (y/N): " choice
    case "$choice" in
      y|Y )
        echo "Installing grcov..."
        cargo install grcov
        if ! command_exists grcov ; then
            echo "Failed to install grcov. Please install it manually and try again."
            exit 1
        fi
        ;;
      * )
        echo "grcov is required to generate coverage reports. Please install it and try again."
        exit 1
        ;;
    esac
fi

# Set environment variables for coverage
# CARGO_INCREMENTAL=0 is recommended for consistent coverage results
export CARGO_INCREMENTAL=0
# RUSTFLAGS tells rustc to instrument the code for coverage
export RUSTFLAGS="-C instrument-coverage"
# LLVM_PROFILE_FILE specifies the pattern for the raw coverage data files
# %p creates a file per process, and %m adds a unique hash per module
export LLVM_PROFILE_FILE="target/coverage/profraws/coverage-%p-%m.profraw"

# Create directories for coverage data and report
echo "Creating coverage directories..."
mkdir -p target/coverage/profraws
mkdir -p target/coverage/report

# Clean and rebuild the project with coverage instrumentation
echo "Cleaning and rebuilding the project with coverage instrumentation..."
cargo clean
cargo build

# Run tests - this generates .profraw files in target/coverage/profraws/
echo "Running tests..."
cargo test

# Generate coverage report using grcov
echo "Generating coverage report..."
grcov ./target/coverage/profraws/ \
    --binary-path ./target/debug/ \
    -s . \
    -t html \
    --ignore-not-existing \
    --ignore "target/*" \
    --ignore "examples/*" \
    --ignore "**/build.rs" \
    --ignore "tests/*" \
    --ignore "rsactor-derive/*" \
    -o ./target/coverage/report/

echo "Coverage report generated at ./target/coverage/report/index.html"
echo "Note: Some files like build scripts or test utility code might be ignored by default."
echo "You might need to adjust grcov arguments (e.g., --ignore patterns) for your specific needs."

# Unset environment variables
unset CARGO_INCREMENTAL
unset RUSTFLAGS
unset LLVM_PROFILE_FILE

rm -rf ./target/debug/*

echo "Done."
