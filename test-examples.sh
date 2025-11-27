#!/bin/bash
# Test script for StartupExamples validation
# Runs unit tests and validation tests for protocol examples

set -e

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║         StartupExamples Validation Tests                       ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo

# Use all features to test ALL protocols
echo "Building with --all-features"
echo

# Run the validation tests
echo "Running startup_examples_validation_test..."
echo "─────────────────────────────────────────────────────────────────"
./cargo-isolated.sh test --all-features --test startup_examples_validation_test -- --test-threads=100 --nocapture

echo
echo "╔════════════════════════════════════════════════════════════════╗"
echo "║                    All Tests Complete                          ║"
echo "╚════════════════════════════════════════════════════════════════╝"
