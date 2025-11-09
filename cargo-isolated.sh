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
# - --print-last: Print the most recent log file for this session

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Print usage if no arguments
if [[ $# -eq 0 ]]; then
    echo "Usage: ./cargo-isolated.sh <cargo-args>" >&2
    echo "       ./cargo-isolated.sh --print-last" >&2
    echo "" >&2
    echo "Examples:" >&2
    echo "  ./cargo-isolated.sh build --no-default-features --features tcp" >&2
    echo "  ./cargo-isolated.sh test --features tcp | tail -50" >&2
    echo "  ./cargo-isolated.sh --print-last | grep 'error'" >&2
    echo "" >&2
    echo "Options:" >&2
    echo "  --print-last     Print the most recent log file for this session" >&2
    echo "  --skip-cleanup   Skip cleanup of old target directories" >&2
    exit 1
fi

# Check for --print-last flag
if [[ "$1" == "--print-last" ]]; then
    TMP_DIR="${SCRIPT_DIR}/tmp"

    # Check if tmp directory exists
    if [[ ! -d "$TMP_DIR" ]]; then
        echo "Error: No tmp/ directory found. Run ./cargo-isolated.sh first." >&2
        exit 1
    fi

    # Use parent shell PID for session tracking
    # This ensures all invocations from the same shell share logs
    SESSION_PID="${CARGO_SESSION_PID:-$PPID}"

    # Find the most recent log file for this session (using session PID)
    # Log files are named: netget-<command>-<pid>.log
    LOG_FILE=$(ls -t "${TMP_DIR}/netget-"*"-${SESSION_PID}.log" 2>/dev/null | head -n 1)

    # Check if any log file was found
    if [[ -z "$LOG_FILE" ]]; then
        echo "Error: No log files found for session PID ${SESSION_PID}" >&2
        echo "Run ./cargo-isolated.sh first to generate logs." >&2
        exit 1
    fi

    # Print info to stderr
    echo "Reading log: $LOG_FILE" >&2
    echo "============================" >&2

    # Cat the log file (stdout can be piped)
    cat "$LOG_FILE"
    exit 0
fi

# Enable isolated build mode and call cargo.sh
# Use parent PID as session PID to track the calling shell
# This ensures all invocations from the same shell session share the same session ID
export CARGO_SESSION_PID="${CARGO_SESSION_PID:-$PPID}"
CARGO_USE_ISOLATION=true exec "${SCRIPT_DIR}/cargo.sh" "$@"
