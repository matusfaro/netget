#!/bin/bash
# Cargo wrapper script with optional isolation support
# Usage: ./cargo.sh <cargo-args>
# Example: ./cargo.sh build --release --all-features
#
# Modes:
# - Standard mode (default): Uses target/ directory
# - Isolated mode: Set CARGO_USE_ISOLATION=true to use target-claude/claude-$$/
#
# Options:
# - --skip-cleanup: Skip cleanup of old target directories (faster startup)

set -e

# Set AWS profile for sccache S3 backend
export RUSTC_WRAPPER=sccache
export SCCACHE_BUCKET=netget-sccache
export SCCACHE_REGION=us-east-1
export SCCACHE_CACHE_SIZE=50G
export AWS_PROFILE=sccache-netget

# Parse flags
SKIP_CLEANUP=false
CARGO_ARGS=()
for arg in "$@"; do
    if [[ "$arg" == "--skip-cleanup" ]]; then
        SKIP_CLEANUP=true
    else
        CARGO_ARGS+=("$arg")
    fi
done

# Determine project root (where Cargo.toml lives)
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Set default values if not provided
CARGO_USE_ISOLATION="${CARGO_USE_ISOLATION:-false}"
CARGO_CLEANUP_OLD="${CARGO_CLEANUP_OLD:-true}"

# Override cleanup if --skip-cleanup flag is passed
if [[ "$SKIP_CLEANUP" == true ]]; then
    CARGO_CLEANUP_OLD=false
fi

# Cleanup old target directories from dead sessions (only for isolated mode)
if [[ "$CARGO_USE_ISOLATION" == true ]] && [[ "$CARGO_CLEANUP_OLD" == true ]] && [[ -d "${PROJECT_ROOT}/target-claude" ]]; then
    CLEANED=0
    for dir in "${PROJECT_ROOT}/target-claude"/claude-*; do
        # Skip if no directories match
        [[ -d "$dir" ]] || continue

        # Extract PID from directory name (e.g., claude-12345 -> 12345)
        PID=$(basename "$dir" | sed 's/^claude-//')

        # Check if PID is still running
        if ! ps -p "$PID" > /dev/null 2>&1; then
            echo "Cleaning up old session: $dir (PID $PID no longer active)" >&2
            rm -rf "$dir"
            ((CLEANED++))
        fi
    done

    if [[ $CLEANED -gt 0 ]]; then
        echo "Cleaned up $CLEANED old session(s)" >&2
    fi
fi

# Set target directory based on isolation mode
if [[ "$CARGO_USE_ISOLATION" == true ]]; then
    # Create session-specific build directory using shell PID
    # $$ is the PID of the current shell, so all invocations within the same
    # terminal session will use the same directory, while different sessions are isolated
    export CARGO_TARGET_DIR="${PROJECT_ROOT}/target-claude/claude-$$"
    mkdir -p "$CARGO_TARGET_DIR"
    BUILD_MODE="Isolated"
else
    # Use standard target directory (don't override CARGO_TARGET_DIR if already set)
    if [[ -z "$CARGO_TARGET_DIR" ]]; then
        export CARGO_TARGET_DIR="${PROJECT_ROOT}/target"
    fi
    BUILD_MODE="Standard"
fi

# Echo the target directory and session info for visibility
echo "=== Cargo $BUILD_MODE Build ===" >&2
if [[ "$CARGO_USE_ISOLATION" == true ]]; then
    echo "Session PID: $$" >&2
fi
echo "Target directory: $CARGO_TARGET_DIR" >&2
echo "Command: cargo ${CARGO_ARGS[*]}" >&2
echo "============================" >&2

# Forward all arguments to cargo (excluding --skip-cleanup flag)
exec cargo "${CARGO_ARGS[@]}"
