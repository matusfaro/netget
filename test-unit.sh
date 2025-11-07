#!/bin/bash
# Unit Test Runner for NetGet
# Runs unit tests (tests that don't require network/Ollama)
#
# Usage:
#   ./test-unit.sh                    # Run all unit tests
#   ./test-unit.sh --verbose          # Run with detailed output
#   ./test-unit.sh --help             # Show this help message
#
# Features:
# - Uses cargo-isolated.sh for build isolation
# - Separates unit tests from E2E tests
# - Provides clear pass/fail reporting

set -e

# Color output for better readability
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Determine project root
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$PROJECT_ROOT"

# Parse arguments
VERBOSE=false
DRY_RUN=false

for arg in "$@"; do
    case "$arg" in
        --verbose|-v)
            VERBOSE=true
            ;;
        --dry-run|-n)
            DRY_RUN=true
            ;;
        --help|-h)
            echo "Unit Test Runner for NetGet"
            echo ""
            echo "Usage:"
            echo "  $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --verbose, -v    Show detailed test output"
            echo "  --dry-run, -n    Show what would be run without executing"
            echo "  --help, -h       Show this help message"
            echo ""
            echo "Examples:"
            echo "  $0                    # Run all unit tests"
            echo "  $0 --verbose          # Run with detailed output"
            echo "  $0 --dry-run          # Preview what would be executed"
            exit 0
            ;;
        -*)
            echo -e "${RED}Error: Unknown option: $arg${NC}" >&2
            echo "Use --help for usage information" >&2
            exit 1
            ;;
        *)
            echo -e "${RED}Error: Unexpected argument: $arg${NC}" >&2
            echo "Use --help for usage information" >&2
            exit 1
            ;;
    esac
done

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}NetGet Unit Test Runner${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""

# Identify unit tests vs E2E tests
# Unit tests: In tests/ directory but NOT in tests/server/*/e2e_test.rs
# E2E tests: In tests/server/*/e2e_test.rs

echo -e "${BLUE}Identifying unit tests...${NC}"
UNIT_TESTS=()

# Find all test files in tests/ excluding E2E tests
for test_file in "$PROJECT_ROOT"/tests/*.rs; do
    if [ -f "$test_file" ]; then
        filename=$(basename "$test_file")
        # Exclude certain files that aren't tests or are integration-only
        if [[ ! "$filename" =~ ^(prompt|server)\.rs$ ]]; then
            UNIT_TESTS+=("$filename")
        fi
    fi
done

if [ ${#UNIT_TESTS[@]} -eq 0 ]; then
    echo -e "${YELLOW}No unit tests found${NC}"
    exit 0
fi

echo -e "${GREEN}Found ${#UNIT_TESTS[@]} unit test file(s):${NC}"
for test in "${UNIT_TESTS[@]}"; do
    echo -e "  ${BLUE}•${NC} $test"
done
echo ""

# Build the test command
TEST_CMD=(
    ./cargo-isolated.sh
    test
    --lib
)

# Add verbose flag if requested
if [ "$VERBOSE" = true ]; then
    TEST_CMD+=(-- --nocapture --test-threads=1)
    echo -e "${BLUE}Running with verbose output...${NC}"
else
    echo -e "${BLUE}Running unit tests...${NC}"
fi

echo ""

# Run the test (or show what would be run in dry-run mode)
if [ "$DRY_RUN" = true ]; then
    echo -e "${YELLOW}[DRY RUN]${NC} Would execute: ${TEST_CMD[*]}"
    echo -e "${YELLOW}[DRY RUN]${NC} Skipping actual test execution"
    exit 0
fi

if "${TEST_CMD[@]}"; then
    echo ""
    echo -e "${BLUE}========================================${NC}"
    echo -e "${GREEN}✓ All unit tests PASSED${NC}"
    echo -e "${BLUE}========================================${NC}"
    exit 0
else
    echo ""
    echo -e "${BLUE}========================================${NC}"
    echo -e "${RED}✗ Some unit tests FAILED${NC}"
    echo -e "${BLUE}========================================${NC}"
    exit 1
fi
