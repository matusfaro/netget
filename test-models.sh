#!/bin/bash
# Ollama Model Benchmarking Script
#
# Runs all Ollama model tests against multiple models and generates
# a comprehensive comparison report.
#
# Usage:
#   ./test-models.sh [model1] [model2] [model3] ...
#
# Example:
#   ./test-models.sh qwen2.5-coder:7b qwen3-coder:30b llama3:8b
#
# Output:
#   - Individual test results saved to ./test-results/<model>.log
#   - Progressive test results printed as they complete
#   - Summary statistics for each model
#

set -euo pipefail

# Default models if none specified
DEFAULT_MODELS=(
    "qwen2.5-coder:7b"
    "qwen3-coder:30b"
    "llama3:8b"
)

# Get models from args or use defaults
if [ $# -eq 0 ]; then
    MODELS=("${DEFAULT_MODELS[@]}")
    echo "ℹ️  No models specified, using defaults: ${MODELS[*]}"
else
    MODELS=("$@")
fi

# Create results directory
RESULTS_DIR="./test-results"
mkdir -p "$RESULTS_DIR"

# Timestamp for this run
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
RUN_DIR="$RESULTS_DIR/run_$TIMESTAMP"
mkdir -p "$RUN_DIR"

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║           Ollama Model Testing Benchmark                       ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""
echo "📊 Testing ${#MODELS[@]} models:"
for model in "${MODELS[@]}"; do
    echo "   • $model"
done
echo ""

# Use ALL features to test against the full netget system
# This ensures we test with ALL protocols and ALL actions (not mocked)
CARGO_FEATURES="--all-features"

# Directory for storing test results (instead of associative array)
RESULTS_CACHE="$RUN_DIR/.results_cache"
mkdir -p "$RESULTS_CACHE"

# Function to store test result (replaces associative array)
store_result() {
    local model="$1"
    local test_name="$2"
    local result="$3"
    local model_safe=$(echo "$model" | tr ':/' '_')
    local test_safe=$(echo "$test_name" | tr ':/' '_')
    echo "$result" > "$RESULTS_CACHE/${model_safe}__${test_safe}"
}

# Function to get test result (replaces associative array lookup)
get_result() {
    local model="$1"
    local test_name="$2"
    local model_safe=$(echo "$model" | tr ':/' '_')
    local test_safe=$(echo "$test_name" | tr ':/' '_')
    local result_file="$RESULTS_CACHE/${model_safe}__${test_safe}"
    if [ -f "$result_file" ]; then
        cat "$result_file"
    else
        echo "⏱️"
    fi
}

echo "Compiling..."
# Compile test binary without running
RUST_LOG=error cargo test $CARGO_FEATURES --test ollama_model_test --no-run 2>&1 | tee "$RUN_DIR/compile.log" | tail -20
compile_exit=${PIPESTATUS[0]}

if [ $compile_exit -ne 0 ]; then
    echo ""
    echo "ERROR: Compilation failed. See $RUN_DIR/compile.log for details"
    exit 1
fi

# Find the compiled test binary
# Try using jq first (more reliable), fallback to manual search
if command -v jq >/dev/null 2>&1; then
    TEST_BINARY=$(cargo test $CARGO_FEATURES --test ollama_model_test --no-run --message-format=json 2>/dev/null | \
        jq -r 'select(.profile.test == true) | select(.executable) | .executable' | \
        grep ollama_model_test | head -1)
else
    # Fallback: find most recently modified test binary
    TEST_BINARY=$(ls -t target/debug/deps/ollama_model_test-* 2>/dev/null | grep -v '\.d$' | head -1)
fi

if [ -z "$TEST_BINARY" ] || [ ! -f "$TEST_BINARY" ]; then
    echo "ERROR: Could not find compiled test binary"
    echo "Expected path matching: target/*/deps/ollama_model_test-*"
    ls -la target/debug/deps/ollama_model_test-* 2>/dev/null || echo "No binaries found"
    exit 1
fi

echo "Test binary: $TEST_BINARY"
echo ""

# Discover tests using the compiled binary
echo "Discovering tests..."
TEST_LIST_OUTPUT=$("$TEST_BINARY" --list 2>/dev/null)
TEST_NAMES=()
while IFS= read -r line; do
    if [[ "$line" =~ ^([a-zA-Z0-9_]+):\ test$ ]]; then
        test_name="${BASH_REMATCH[1]}"
        TEST_NAMES+=("$test_name")
    fi
done <<< "$TEST_LIST_OUTPUT"

echo "Found ${#TEST_NAMES[@]} tests"
echo ""
echo "Running tests sequentially for accurate benchmarking..."
echo "Testing same test across all models for direct comparison"
echo ""

# Single log file for all tests
LOG_FILE="$RUN_DIR/test_results.log"
echo "Log file: $LOG_FILE"
echo ""

# Print table header
echo "╔════════════════════════════════════════════════════════════════╗"
echo "║                     RESULTS TABLE                              ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Print column headers
printf "%-40s" "Test Name"
for model in "${MODELS[@]}"; do
    # Show last 12 chars which includes the param count
    model_short=$(echo "$model" | rev | cut -c1-12 | rev)
    printf " │ %-12s" "$model_short"
done
echo ""

# Print separator
printf "%-40s" "$(printf '─%.0s' {1..40})"
for model in "${MODELS[@]}"; do
    printf "─┼─%-12s" "$(printf '─%.0s' {1..12})"
done
echo ""

# Run each test across all models (for direct comparison)
for test_name in "${TEST_NAMES[@]}"; do
    # Shorten test name for display
    test_display=$(echo "$test_name" | sed 's/^test_//' | cut -c1-38)

    # Print test name and separator
    printf "%-40s │" "$test_display"

    # Flush output to show the row immediately
    printf ""

    # Run this test for each model and print results in columns
    for model in "${MODELS[@]}"; do
        # Print header to log file
        echo "" >> "$LOG_FILE"
        echo "═══════════════════════════════════════════════════════════════" >> "$LOG_FILE"
        echo "Model: $model" >> "$LOG_FILE"
        echo "Test:  $test_name" >> "$LOG_FILE"
        echo "═══════════════════════════════════════════════════════════════" >> "$LOG_FILE"
        echo "" >> "$LOG_FILE"

        # Run single test for this model with 120 second timeout
        # Use the pre-compiled binary directly (no recompilation!)
        # Enable trace logging to see all Ollama calls
        RUST_LOG=trace OLLAMA_MODEL="$model" "$TEST_BINARY" "$test_name" --nocapture >> "$LOG_FILE" 2>&1 &
        test_pid=$!

        # Wait for test with timeout (120 seconds = 2 minutes)
        timeout_seconds=120
        elapsed=0
        while kill -0 $test_pid 2>/dev/null; do
            sleep 1
            ((elapsed++))
            if [ $elapsed -ge $timeout_seconds ]; then
                # Timeout - kill the test
                echo "" >> "$LOG_FILE"
                echo "⏱️  TIMEOUT after ${timeout_seconds}s" >> "$LOG_FILE"
                echo "" >> "$LOG_FILE"
                kill -9 $test_pid 2>/dev/null
                wait $test_pid 2>/dev/null
                test_exit_code=124  # Standard timeout exit code
                break
            fi
        done

        # Get exit code if not timed out
        if [ $elapsed -lt $timeout_seconds ]; then
            # Use set +e temporarily to capture exit code without exiting script
            set +e
            wait $test_pid
            test_exit_code=$?
            set -e
        fi

        # Store and print result
        # Match header format exactly: " │ %-12s" where emoji(2) + 10 spaces = 12 visual chars
        if [ $test_exit_code -eq 0 ]; then
            store_result "$model" "$test_name" "✅"
            printf " │ ✅          "
        elif [ $test_exit_code -eq 124 ]; then
            store_result "$model" "$test_name" "⏱️"
            printf " │ ⏱️          "
        else
            store_result "$model" "$test_name" "❌"
            printf " │ ❌          "
        fi
    done

    echo ""
done

echo ""
echo "╔════════════════════════════════════════════════════════════════╗"
echo "║                     FINAL RESULTS TABLE                        ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Print header with model names on separate lines for better alignment
echo "Test Name                                          │ Results"
echo "───────────────────────────────────────────────────┼────────────────────────"

# Print each model name on its own line
for model in "${MODELS[@]}"; do
    model_short=$(echo "$model" | rev | cut -c1-20 | rev)
    echo "                                                   │ $model_short"
done
echo "───────────────────────────────────────────────────┼────────────────────────"

# Print test results
for test_name in "${TEST_NAMES[@]}"; do
    # Shorten test name (remove test_ prefix)
    test_display=$(echo "$test_name" | sed 's/^test_//' | cut -c1-48)
    printf "%-50s │" "$test_display"

    for model in "${MODELS[@]}"; do
        result=$(get_result "$model" "$test_name")
        printf " %-3s" "$result"
    done
    echo ""
done

echo ""
echo "╔════════════════════════════════════════════════════════════════╗"
echo "║                        SUMMARY                                 ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Calculate summary statistics
for model in "${MODELS[@]}"; do
    passed=0
    failed=0

    for test_name in "${TEST_NAMES[@]}"; do
        result=$(get_result "$model" "$test_name")
        if [ "$result" = "✅" ]; then
            ((passed++)) || true
        else
            ((failed++)) || true
        fi
    done

    total=$((passed + failed))
    if [ $total -gt 0 ]; then
        success_rate=$((passed * 100 / total))
    else
        success_rate=0
    fi

    echo "🤖 $model"
    echo "   ✅ Passed: $passed/$total ($success_rate%)"
    echo "   ❌ Failed: $failed/$total"
    echo ""
done

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║                     DETAILED LOG                               ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""
echo "📄 Complete test log: $LOG_FILE"
echo ""

# Count failures in the log
failure_count=$(grep -c "Error: Test failed" "$LOG_FILE" 2>/dev/null || echo "0")
echo "   Total assertion failures: $failure_count"
echo ""

echo "The log file contains:"
echo "  • Headers for each model/test combination"
echo "  • Full cargo test output"
echo "  • LLM responses for each test"
echo "  • Specific assertion failures with expected vs actual values"
echo "  • Stack traces for panics"
echo ""
echo "╔════════════════════════════════════════════════════════════════╗"
echo "║                       COMPLETE                                 ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""
echo "✨ Benchmark complete!"
echo "📊 Results table: See above"
echo "📂 All files saved to: $RUN_DIR"
echo "📝 Markdown report: $RUN_DIR/REPORT.md"
echo ""

# Generate a markdown report
REPORT_FILE="$RUN_DIR/REPORT.md"

cat > "$REPORT_FILE" <<EOF
# Ollama Model Benchmark Report

**Date**: $(date)
**Models Tested**: ${#MODELS[@]}
**Tests Run**: ${#TEST_NAMES[@]}

## Results Table

| Test Name | $(printf "%s | " "${MODELS[@]}") |
|-----------|$(printf -- "------------|%.0s" "${MODELS[@]}")---|
EOF

for test_name in "${TEST_NAMES[@]}"; do
    test_display=$(echo "$test_name" | sed 's/^test_//')
    echo -n "| $test_display |" >> "$REPORT_FILE"
    for model in "${MODELS[@]}"; do
        result=$(get_result "$model" "$test_name")
        echo -n " $result |" >> "$REPORT_FILE"
    done
    echo "" >> "$REPORT_FILE"
done

cat >> "$REPORT_FILE" <<EOF

**Legend**: ✅ = Passed, ❌ = Failed, ⏱️ = Timeout/Pending

EOF

cat >> "$REPORT_FILE" <<EOF

## Summary

EOF

for model in "${MODELS[@]}"; do
    passed=0
    failed=0

    for test_name in "${TEST_NAMES[@]}"; do
        result=$(get_result "$model" "$test_name")
        if [ "$result" = "✅" ]; then
            ((passed++)) || true
        else
            ((failed++)) || true
        fi
    done

    total=$((passed + failed))
    if [ $total -gt 0 ]; then
        success_rate=$((passed * 100 / total))
    else
        success_rate=0
    fi

    cat >> "$REPORT_FILE" <<EOF
### $model

- **Passed**: $passed/$total ($success_rate%)
- **Failed**: $failed/$total

EOF
done

# Count total failures
failure_count=$(grep -c "Error: Test failed" "$LOG_FILE" 2>/dev/null || echo "0")

cat >> "$REPORT_FILE" <<EOF

## Detailed Log

The complete test log contains all test runs with headers for each model/test combination.

- **Log File**: [test_results.log](./test_results.log)
- **Total Assertion Failures**: $failure_count

The log file includes:
- Headers showing which model and test is running
- Full cargo test output
- LLM responses for each test
- Specific assertion failures with expected vs actual values
- Stack traces for any panics

EOF
