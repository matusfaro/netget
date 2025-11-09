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

# Enable isolated build mode and call cargo.sh
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CARGO_USE_ISOLATION=true exec "${SCRIPT_DIR}/cargo.sh" "$@"
