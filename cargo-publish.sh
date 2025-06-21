#!/bin/bash

set -e  # Exit on any error

# rsactor-derive must be published first since rsactor depends on it
declare -a publish_dirs=("rsactor-derive" ".")

function check_build() {
    echo "Checking if all packages build successfully..."
    for dir in "${publish_dirs[@]}"; do
        echo "Building $dir..."
        pushd "$dir" > /dev/null
        if ! cargo build; then
            echo "❌ Build failed for $dir"
            popd > /dev/null
            return 1
        fi
        popd > /dev/null
        echo "✅ Build successful for $dir"
    done
    echo "All packages build successfully!"
}

function wait_for_crate() {
    local crate_name="$1"
    local version="$2"
    local max_attempts=30
    local attempt=0

    echo "Waiting for $crate_name v$version to be available on crates.io..."

    while [ $attempt -lt $max_attempts ]; do
        if cargo search "$crate_name" --limit 1 | grep -q "$crate_name.*$version"; then
            echo "✅ $crate_name v$version is now available on crates.io"
            return 0
        fi

        attempt=$((attempt + 1))
        echo "Attempt $attempt/$max_attempts: Waiting for $crate_name to be available..."
        sleep 10
    done

    echo "⚠️  Warning: $crate_name v$version may not be available yet, continuing anyway..."
    return 0
}

function publish() {
    local dry_run=false
    local cargo_options=()

    # Parse arguments
    for arg in "$@"; do
        case $arg in
            --dry-run)
                dry_run=true
                cargo_options+=("$arg" "--allow-dirty")
                ;;
            --help|-h)
                echo "Usage: $0 [--dry-run] [--help]"
                echo "  --dry-run    Perform a dry run without actually publishing"
                echo "  --help, -h   Show this help message"
                return 0
                ;;
            *)
                echo "Unknown option: $arg"
                return 1
                ;;
        esac
    done

    # Check if all packages build before publishing
    if ! check_build; then
        echo "❌ Build check failed. Aborting publish."
        return 1
    fi

    # Get version from workspace
    local version=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
    echo "Publishing version: $version"

    for i in "${!publish_dirs[@]}"; do
        local dir="${publish_dirs[$i]}"
        echo ""
        echo "📦 Publishing $dir..."

        pushd "$dir" > /dev/null

        # Show what will be published
        echo "Contents to be published:"
        cargo package --list | head -10

        if [ $dry_run = true ]; then
            echo "🔍 Dry run mode - not actually publishing"
        else
            echo "Are you sure you want to publish $dir? (y/N)"
            read -r confirmation
            if [[ ! "$confirmation" =~ ^[Yy]$ ]]; then
                echo "Skipping $dir"
                popd > /dev/null
                continue
            fi
        fi

        if ! cargo publish "${cargo_options[@]}"; then
            echo "❌ Error occurred while publishing $dir"
            popd > /dev/null
            return 1
        fi

        popd > /dev/null

        if [ $dry_run = false ]; then
            echo "✅ Successfully published $dir"

            # Wait for the crate to be available before publishing the next one
            # (except for the last crate)
            if [ $i -lt $((${#publish_dirs[@]} - 1)) ]; then
                wait_for_crate "$dir" "$version"
            fi
        else
            echo "✅ Dry run completed for $dir"
        fi
    done

    if [ $dry_run = false ]; then
        echo ""
        echo "🎉 All packages published successfully!"
        echo "You can check the published packages at:"
        for dir in "${publish_dirs[@]}"; do
            echo "  https://crates.io/crates/$dir"
        done
    else
        echo ""
        echo "🔍 Dry run completed for all packages"
    fi

    return 0
}

# Main execution
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    publish "$@"
fi