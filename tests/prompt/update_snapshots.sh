#!/bin/bash

# Script to update prompt test snapshots
# This script will:
# 1. Run the prompt tests to generate .actual.snap.md files
# 2. Update all snapshots with the new versions

SNAPSHOTS_DIR="$(dirname "$0")/snapshots"
SCRIPT_DIR="$(dirname "$0")"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Prompt Snapshot Update Script"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Step 1: Run tests to generate .actual.snap.md files
echo "Step 1: Running prompt tests to generate snapshots..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Run from repo root (2 levels up from tests/prompt/)
cd "$SCRIPT_DIR/../.."

# Run tests and capture exit code
set +e  # Don't exit on error
cargo test --test prompt
test_exit_code=$?
set -e  # Re-enable exit on error

if [ $test_exit_code -eq 0 ]; then
    echo ""
    echo "✓ All tests already passing - no snapshots to update"
    exit 0
fi

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Step 2: Count and update snapshots
echo "Step 2: Updating snapshots..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

count=$(find "$SNAPSHOTS_DIR" -name "*.actual.snap.md" 2>/dev/null | wc -l | tr -d ' ')

if [ "$count" -eq 0 ]; then
    echo "⚠ No .actual.snap.md files found - tests may have all passed"
    exit 0
fi

echo "Found $count snapshot(s) to update:"
echo ""

# Move each .actual.snap.md file to replace the .snap.md file
for actual_file in "$SNAPSHOTS_DIR"/*.actual.snap.md; do
    if [ -f "$actual_file" ]; then
        snap_file="${actual_file%.actual.snap.md}.snap.md"
        filename=$(basename "$snap_file")

        echo "  → Updating $filename"
        mv "$actual_file" "$snap_file"
    fi
done

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "✓ Snapshots updated successfully!"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
