#!/bin/bash
# Cargo wrapper script for isolated builds across multiple Claude instances
# Usage: ./cargo-isolated.sh <cargo-args>
# Example: ./cargo-isolated.sh build --release --all-features
#
# How it works:
# - Uses $$ (shell PID) to create session-specific build directories
# - All cargo commands in the same terminal session share the same build directory
# - Different terminal sessions (different Claude instances) get isolated directories
# - Format: target-claude/claude-{shell_pid}/
#
# Options:
# - --skip-cleanup: Skip cleanup of old target directories (faster startup)

set -e

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

# Cleanup old target directories from dead sessions
if [[ "$SKIP_CLEANUP" == false ]] && [[ -d "${PROJECT_ROOT}/target-claude" ]]; then
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

# Create session-specific build directory using shell PID
# $$ is the PID of the current shell, so all invocations within the same
# terminal session will use the same directory, while different sessions are isolated
export CARGO_TARGET_DIR="${PROJECT_ROOT}/target-claude/claude-$$"

# Create directory if it doesn't exist
mkdir -p "$CARGO_TARGET_DIR"

# Echo the target directory and session info for visibility
echo "=== Cargo Isolated Build ===" >&2
echo "Session PID: $$" >&2
echo "Target directory: $CARGO_TARGET_DIR" >&2
echo "Command: cargo ${CARGO_ARGS[*]}" >&2
echo "============================" >&2

# Forward all arguments to cargo (excluding --skip-cleanup flag)
exec cargo "${CARGO_ARGS[@]}"
