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

set -e

# Determine project root (where Cargo.toml lives)
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Create session-specific build directory using shell PID
# $$ is the PID of the current shell, so all invocations within the same
# terminal session will use the same directory, while different sessions are isolated
export CARGO_TARGET_DIR="${PROJECT_ROOT}/target-claude/claude-$$"

# Create directory if it doesn't exist
mkdir -p "$CARGO_TARGET_DIR"

# Echo the target directory for visibility
echo "Using isolated build directory: $CARGO_TARGET_DIR" >&2

# Forward all arguments to cargo
exec cargo "$@"
