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

# Check for sccache
if [[ "$RUSTC_WRAPPER" == "sccache" ]]; then
    # RUSTC_WRAPPER is set to sccache - verify it's installed
    if ! command -v sccache &> /dev/null; then
        echo "⚠️  RUSTC_WRAPPER is set to sccache, but sccache is not installed" >&2
        echo "Installing sccache automatically..." >&2
        echo "" >&2

        # Install sccache
        if cargo install sccache; then
            echo "" >&2
            echo "✓ sccache installed successfully" >&2
            echo "" >&2
        else
            echo "" >&2
            echo "❌ Failed to install sccache" >&2
            echo "Build may fail. To fix manually:" >&2
            echo "  cargo install sccache" >&2
            echo "Or disable sccache for this session:" >&2
            echo "  unset RUSTC_WRAPPER" >&2
            echo "" >&2
            exit 1
        fi
    fi
elif [[ -z "$RUSTC_WRAPPER" || "$RUSTC_WRAPPER" != "sccache" ]]; then
    # sccache not configured - show optional warning
    echo "⚠️  WARNING: RUSTC_WRAPPER is not set to sccache" >&2
    echo "" >&2
    echo "To speed up builds, install sccache:" >&2
    echo "  cargo install sccache" >&2
    echo "" >&2
    echo "Then add to your ~/.bashrc, ~/.zshrc, or shell config:" >&2
    echo "  export RUSTC_WRAPPER=sccache" >&2
    echo "  export SCCACHE_CACHE_SIZE=50G" >&2
    echo "" >&2
    echo "Or set it for this session:" >&2
    echo "  export RUSTC_WRAPPER=sccache" >&2
    echo "  export SCCACHE_CACHE_SIZE=50G" >&2
    echo "" >&2
fi

# Create tmp directory for logs
mkdir -p "${PROJECT_ROOT}/tmp"

# Determine log file name based on command (first argument)
COMMAND="${CARGO_ARGS[0]:-unknown}"
LOG_FILE="${PROJECT_ROOT}/tmp/netget-${COMMAND}-$$.log"

# Echo the target directory and session info for visibility
echo "=== Cargo $BUILD_MODE Build ===" >&2
if [[ "$CARGO_USE_ISOLATION" == true ]]; then
    echo "Session PID: $$" >&2
fi
echo "Target directory: $CARGO_TARGET_DIR" >&2
echo "Command: cargo ${CARGO_ARGS[*]}" >&2
echo "Log file: $LOG_FILE" >&2
echo "============================" >&2

# Enable pipefail to capture cargo's exit code through the pipe
set -o pipefail

# Run cargo and tee output to log file (captures both stdout and stderr)
cargo "${CARGO_ARGS[@]}" 2>&1 | tee "$LOG_FILE"
EXIT_CODE=$?

# Save the log file path for later retrieval
echo "$LOG_FILE" > "${PROJECT_ROOT}/tmp/last-log.txt"

# Exit with cargo's exit code
exit $EXIT_CODE
