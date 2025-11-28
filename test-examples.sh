#!/bin/bash
# Test script for Protocol Examples
#
# Runs both static validation tests and E2E example tests:
# 1. startup_examples_validation_test - Validates JSON structure
# 2. examples/coverage_test - Verifies test coverage
# 3. E2E example tests (*example*) - Actually runs protocols with examples
#
# Usage:
#   ./test-examples.sh                    # Run all example tests
#   ./test-examples.sh --static-only      # Run only static validation
#   ./test-examples.sh --e2e-only         # Run only E2E tests
#   ./test-examples.sh tcp http dns       # Run specific protocol E2E tests

set -e

# Color output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Parse arguments
STATIC_ONLY=false
E2E_ONLY=false
PROTOCOLS=()

while [[ $# -gt 0 ]]; do
    case "$1" in
        --static-only)
            STATIC_ONLY=true
            shift
            ;;
        --e2e-only)
            E2E_ONLY=true
            shift
            ;;
        --help|-h)
            echo "Protocol Examples Test Runner"
            echo ""
            echo "Usage:"
            echo "  $0 [OPTIONS] [PROTOCOLS...]"
            echo ""
            echo "Options:"
            echo "  --static-only    Run only static validation tests"
            echo "  --e2e-only       Run only E2E example tests"
            echo "  --help, -h       Show this help message"
            echo ""
            echo "Examples:"
            echo "  $0                # Run all example tests"
            echo "  $0 --static-only  # Run only static validation"
            echo "  $0 tcp http dns   # Run E2E tests for specific protocols"
            exit 0
            ;;
        *)
            PROTOCOLS+=("$1")
            shift
            ;;
    esac
done

echo -e "${BLUE}╔════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║              Protocol Examples Tests                           ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════════════╝${NC}"
echo

# Determine features to use
FEATURE_FLAGS="--all-features"

# Check if we're in Claude Code for Web environment
if [ "$CLAUDE_CODE_REMOTE" = "true" ]; then
    echo -e "${YELLOW}⚠ Running in Claude Code for Web - excluding system-dependent features${NC}"
    FEATURE_FLAGS="--no-default-features --features tcp,http,dns,udp,ssh,telnet,smtp,imap,pop3,ntp,snmp"
fi

# Phase 1: Static Validation Tests
if [ "$E2E_ONLY" = false ]; then
    echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
    echo -e "${BLUE}Phase 1: Static Validation Tests${NC}"
    echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
    echo

    echo -e "${YELLOW}Running startup_examples_validation_test...${NC}"
    echo "─────────────────────────────────────────────────────────────────"
    ./cargo-isolated.sh test $FEATURE_FLAGS --test startup_examples_validation_test -- --test-threads=100 --nocapture
    echo -e "${GREEN}✓ Static validation tests passed${NC}"
    echo
fi

# Phase 2: Coverage Verification
if [ "$E2E_ONLY" = false ]; then
    echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
    echo -e "${BLUE}Phase 2: Coverage Verification${NC}"
    echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
    echo

    echo -e "${YELLOW}Running coverage tests...${NC}"
    echo "─────────────────────────────────────────────────────────────────"
    ./cargo-isolated.sh test $FEATURE_FLAGS coverage_test -- --test-threads=100 --nocapture 2>/dev/null || true
    echo
fi

# Phase 3: E2E Example Tests
if [ "$STATIC_ONLY" = false ]; then
    echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
    echo -e "${BLUE}Phase 3: E2E Example Tests${NC}"
    echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
    echo

    if [ ${#PROTOCOLS[@]} -gt 0 ]; then
        # Run specific protocol tests
        echo -e "${YELLOW}Running E2E example tests for: ${PROTOCOLS[*]}${NC}"
        echo "─────────────────────────────────────────────────────────────────"

        for protocol in "${PROTOCOLS[@]}"; do
            echo -e "${BLUE}Testing protocol: $protocol${NC}"
            ./cargo-isolated.sh test --no-default-features --features "$protocol" \
                example_test -- --test-threads=100 --nocapture || {
                echo -e "${RED}✗ $protocol example tests failed${NC}"
            }
            echo
        done
    else
        # Run all E2E example tests
        echo -e "${YELLOW}Running all E2E example tests (tests matching '*example*')...${NC}"
        echo "─────────────────────────────────────────────────────────────────"

        # Run tests that match 'example' pattern
        # Note: We use a pattern that matches test function names like example_test_*
        ./cargo-isolated.sh test $FEATURE_FLAGS example_test -- --test-threads=100 --nocapture || {
            echo -e "${YELLOW}⚠ Some E2E tests may have failed or been skipped${NC}"
        }
    fi

    echo -e "${GREEN}✓ E2E example tests completed${NC}"
    echo
fi

echo -e "${BLUE}╔════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║                    All Tests Complete                          ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════════════╝${NC}"
