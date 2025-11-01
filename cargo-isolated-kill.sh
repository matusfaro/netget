#!/bin/bash
# Safely kill only YOUR cargo/rustc processes from this session's isolated build
# Usage: ./cargo-isolated-kill.sh
#
# This script only kills processes associated with the current shell session's
# isolated build directory (target-claude/claude-$$). It will NOT affect other
# Claude instances or their build processes.

set -e

# Determine project root
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Get the session-specific build directory (same as cargo-isolated.sh)
SESSION_BUILD_DIR="${PROJECT_ROOT}/target-claude/claude-$$"

echo "=== Cargo Isolated Kill ===" >&2
echo "Session PID: $$" >&2
echo "Session build directory: $SESSION_BUILD_DIR" >&2
echo "============================" >&2

# Check if the build directory exists
if [ ! -d "$SESSION_BUILD_DIR" ]; then
    echo "No build directory found for this session. Nothing to kill." >&2
    exit 0
fi

# Find all cargo and rustc processes that are using this session's build directory
# We check the command line arguments for the CARGO_TARGET_DIR path
PIDS=$(ps -eo pid,command | grep -E "(cargo|rustc)" | grep "$SESSION_BUILD_DIR" | grep -v "grep" | awk '{print $1}' || true)

if [ -z "$PIDS" ]; then
    echo "No cargo/rustc processes found for this session." >&2
    exit 0
fi

echo "Found the following processes to kill:" >&2
ps -p $PIDS -o pid,command 2>/dev/null || true

# Ask for confirmation
read -p "Kill these processes? [y/N] " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "Aborted." >&2
    exit 0
fi

# Kill the processes
for PID in $PIDS; do
    echo "Killing process $PID..." >&2
    kill $PID 2>/dev/null || echo "  (Process $PID already terminated)" >&2
done

echo "Done. Killed $( echo "$PIDS" | wc -w | tr -d ' ') process(es)." >&2
