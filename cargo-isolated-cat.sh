#!/bin/bash
# Print the last cargo build/test log
# Usage: ./cargo-isolated-cat.sh [args...]
# Example: ./cargo-isolated-cat.sh | tail -100
# Example: ./cargo-isolated-cat.sh | grep "error\[E"

set -e

# Determine project root
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LAST_LOG_FILE="${PROJECT_ROOT}/tmp/last-log.txt"

# Check if last log file tracking exists
if [[ ! -f "$LAST_LOG_FILE" ]]; then
    echo "Error: No log file found. Run ./cargo-isolated.sh first." >&2
    exit 1
fi

# Read the log file path
LOG_FILE=$(cat "$LAST_LOG_FILE")

# Check if the log file exists
if [[ ! -f "$LOG_FILE" ]]; then
    echo "Error: Log file not found: $LOG_FILE" >&2
    echo "The log may have been deleted or moved." >&2
    exit 1
fi

# Print info to stderr
echo "Reading log: $LOG_FILE" >&2
echo "============================" >&2

# Cat the log file (stdout can be piped)
cat "$LOG_FILE"
