#!/bin/bash
# Snapshot Test Runner for NetGet
# Updates and validates snapshot tests with user confirmation
#
# Usage:
#   ./test-snapshot.sh                     # Run all snapshot tests
#   ./test-snapshot.sh prompt              # Run specific snapshot test (prompt)
#   ./test-snapshot.sh --auto-accept       # Accept all changes without asking
#   ./test-snapshot.sh --help              # Show this help message
#
# Features:
# - Runs snapshot tests to generate .actual.snap.md files
# - Shows diffs for all changed snapshots
# - Asks for user confirmation before accepting changes
# - Supports auto-accept mode for CI/automation

set -e

# Color output for better readability
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
MAGENTA='\033[0;35m'
NC='\033[0m' # No Color

# Determine project root
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$PROJECT_ROOT"

# Parse arguments
AUTO_ACCEPT=false
SNAPSHOT_TYPES=()

for arg in "$@"; do
    case "$arg" in
        --auto-accept)
            AUTO_ACCEPT=true
            ;;
        --help|-h)
            echo "Snapshot Test Runner for NetGet"
            echo ""
            echo "Usage:"
            echo "  $0 [OPTIONS] [TEST_TYPES...]"
            echo ""
            echo "Options:"
            echo "  --auto-accept    Accept all snapshot changes without asking"
            echo "  --help, -h       Show this help message"
            echo ""
            echo "Available snapshot test types:"
            echo "  prompt           Prompt generation snapshots"
            echo ""
            echo "Examples:"
            echo "  $0                       # Run all snapshot tests"
            echo "  $0 prompt                # Run only prompt snapshots"
            echo "  $0 --auto-accept         # Accept all changes automatically"
            exit 0
            ;;
        -*)
            echo -e "${RED}Error: Unknown option: $arg${NC}" >&2
            echo "Use --help for usage information" >&2
            exit 1
            ;;
        *)
            SNAPSHOT_TYPES+=("$arg")
            ;;
    esac
done

# If no snapshot types specified, run all
if [ ${#SNAPSHOT_TYPES[@]} -eq 0 ]; then
    SNAPSHOT_TYPES=("prompt")
fi

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}NetGet Snapshot Test Runner${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""

# Validate requested snapshot types
VALID_TYPES=("prompt")
INVALID_TYPES=()
for type in "${SNAPSHOT_TYPES[@]}"; do
    if [[ ! " ${VALID_TYPES[@]} " =~ " ${type} " ]]; then
        INVALID_TYPES+=("$type")
    fi
done

if [ ${#INVALID_TYPES[@]} -gt 0 ]; then
    echo -e "${RED}Error: Unknown snapshot test type(s):${NC}" >&2
    for type in "${INVALID_TYPES[@]}"; do
        echo -e "  ${RED}✗${NC} $type" >&2
    done
    echo "" >&2
    echo "Valid types: ${VALID_TYPES[*]}" >&2
    exit 1
fi

# Track changed snapshots (use parallel arrays for bash 3.2 compatibility)
SNAP_FILES=()
ACTUAL_FILES=()
SNAPSHOT_COUNT=0
ALL_PASSED=true

# Run tests for each snapshot type
for snapshot_type in "${SNAPSHOT_TYPES[@]}"; do
    echo -e "${BLUE}Running ${YELLOW}$snapshot_type${BLUE} snapshot tests...${NC}"

    # Map snapshot type names to test target names
    case "$snapshot_type" in
        prompt)
            test_target="prompt_snapshots"
            ;;
    esac

    # Run tests (don't exit on failure)
    set +e
    ./cargo-isolated.sh test --test "$test_target" 2>&1 | grep -v "^warning:" || true
    test_exit=$?
    set -e

    if [ $test_exit -eq 0 ]; then
        echo -e "${GREEN}✓${NC} No snapshot changes for ${CYAN}$snapshot_type${NC}"
        echo ""
        continue
    fi

    ALL_PASSED=false

    # Find .actual.snap.md files
    case "$snapshot_type" in
        prompt)
            SNAPSHOTS_DIR="$PROJECT_ROOT/tests/prompt_snapshots/snapshots"
            ;;
    esac

    # Check for actual snapshot files
    if [ ! -d "$SNAPSHOTS_DIR" ]; then
        echo -e "${YELLOW}⚠${NC} Snapshots directory not found: ${CYAN}$SNAPSHOTS_DIR${NC}"
        echo ""
        continue
    fi

    # Find all changed snapshots
    ACTUAL_FILES=$(find "$SNAPSHOTS_DIR" -name "*.actual.snap.md" -type f 2>/dev/null | sort)

    if [ -z "$ACTUAL_FILES" ]; then
        echo -e "${YELLOW}⚠${NC} No snapshot changes detected (files may have same content)"
        echo ""
        continue
    fi

    echo -e "${YELLOW}Found snapshot changes for ${CYAN}$snapshot_type${YELLOW}:${NC}"
    echo ""

    # Process each changed snapshot
    while IFS= read -r actual_file; do
        snap_file="${actual_file%.actual.snap.md}.snap.md"
        filename=$(basename "$snap_file")
        SNAPSHOT_COUNT=$((SNAPSHOT_COUNT + 1))

        SNAP_FILES+=("$snap_file")
        ACTUAL_FILES+=("$actual_file")

        echo -e "${MAGENTA}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
        echo -e "${CYAN}Snapshot: $filename${NC}"
        echo -e "${MAGENTA}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
        echo ""

        if [ -f "$snap_file" ]; then
            # Show diff
            echo -e "${YELLOW}Changes:${NC}"
            echo ""
            diff -u "$snap_file" "$actual_file" 2>/dev/null || true
        else
            # New snapshot
            echo -e "${GREEN}New snapshot${NC}"
            echo ""
            head -20 "$actual_file" || true
            if [ $(wc -l < "$actual_file") -gt 20 ]; then
                echo ""
                echo -e "${YELLOW}... (truncated, see full file for details) ...${NC}"
            fi
        fi
        echo ""
    done <<< "$ACTUAL_FILES"
done

echo ""
echo -e "${BLUE}========================================${NC}"

# Handle results
if [ $SNAPSHOT_COUNT -eq 0 ]; then
    echo -e "${GREEN}✓ All snapshots are up to date!${NC}"
    echo -e "${BLUE}========================================${NC}"
    exit 0
fi

# Ask for confirmation or use auto-accept
if [ "$AUTO_ACCEPT" = true ]; then
    echo -e "${YELLOW}Auto-accept mode: accepting ${SNAPSHOT_COUNT} snapshot change(s)${NC}"
    USER_RESPONSE="y"
else
    echo -e "${YELLOW}Review complete. Accept ${SNAPSHOT_COUNT} snapshot change(s)?${NC}"
    echo ""
    echo -ne "${CYAN}Accept changes? [y/n]: ${NC}"
    read -r USER_RESPONSE
    echo ""
fi

if [[ "$USER_RESPONSE" =~ ^[Yy]$ ]]; then
    echo -e "${BLUE}Accepting snapshot changes...${NC}"
    echo ""

    ACCEPTED_COUNT=0
    for i in "${!SNAP_FILES[@]}"; do
        snap_file="${SNAP_FILES[$i]}"
        actual_file="${ACTUAL_FILES[$i]}"
        filename=$(basename "$snap_file")

        mv "$actual_file" "$snap_file"
        echo -e "  ${GREEN}✓${NC} $filename"
        ACCEPTED_COUNT=$((ACCEPTED_COUNT + 1))
    done

    echo ""
    echo -e "${BLUE}========================================${NC}"
    echo -e "${GREEN}✓ Successfully updated ${ACCEPTED_COUNT} snapshot(s)!${NC}"
    echo -e "${BLUE}========================================${NC}"
    exit 0
else
    echo -e "${BLUE}Rejecting snapshot changes.${NC}"
    echo -e "${YELLOW}To accept changes, run: ${CYAN}$0 --auto-accept${NC}"
    echo ""
    echo -e "${BLUE}========================================${NC}"
    exit 1
fi
