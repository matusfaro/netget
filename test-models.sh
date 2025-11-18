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
#   - Comparison table printed to stdout
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
echo "📊 Testing ${#MODELS[@]} models"
echo "📂 Results will be saved to: $RUN_DIR"
echo ""

# Array to store test names
declare -a TEST_NAMES
declare -A TEST_RESULTS  # model -> test -> result (PASS/FAIL)

# Function to extract test names from cargo test output
extract_test_names() {
    local log_file="$1"
    grep "^test " "$log_file" | sed 's/^test //' | sed 's/ \.\.\. .*//' | sort -u
}

# Function to check if a test passed
test_passed() {
    local log_file="$1"
    local test_name="$2"
    grep -q "^test $test_name \.\.\. ok$" "$log_file"
}

# Run tests for each model
for model in "${MODELS[@]}"; do
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "🤖 Testing model: $model"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    # Sanitize model name for filename
    model_filename=$(echo "$model" | tr ':/' '_')
    log_file="$RUN_DIR/${model_filename}.log"

    # Run tests with the model
    echo "⏳ Running tests (this may take a few minutes)..."

    # Set OLLAMA_MODEL and run tests, capture output
    if OLLAMA_MODEL="$model" cargo test --test ollama_model_test -- --nocapture 2>&1 | tee "$log_file"; then
        test_status="✅ PASSED"
    else
        test_status="❌ FAILED"
    fi

    echo ""
    echo "Status: $test_status"
    echo "📄 Results: $log_file"
    echo ""

    # Extract test names from the first model (they're all the same)
    if [ ${#TEST_NAMES[@]} -eq 0 ]; then
        mapfile -t TEST_NAMES < <(extract_test_names "$log_file")
        echo "📋 Found ${#TEST_NAMES[@]} tests"
    fi

    # Store results for each test
    for test_name in "${TEST_NAMES[@]}"; do
        if test_passed "$log_file" "$test_name"; then
            TEST_RESULTS["${model}__${test_name}"]="✅"
        else
            TEST_RESULTS["${model}__${test_name}"]="❌"
        fi
    done
done

echo ""
echo "╔════════════════════════════════════════════════════════════════╗"
echo "║                     COMPARISON REPORT                          ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Generate comparison table
# Header
printf "%-50s" "Test Name"
for model in "${MODELS[@]}"; do
    # Truncate model name if too long
    model_short=$(echo "$model" | cut -c1-15)
    printf " | %-15s" "$model_short"
done
echo ""

# Separator
printf "%-50s" "$(printf '─%.0s' {1..50})"
for model in "${MODELS[@]}"; do
    printf " | %-15s" "$(printf '─%.0s' {1..15})"
done
echo ""

# Test rows
for test_name in "${TEST_NAMES[@]}"; do
    # Shorten test name (remove test_ prefix, truncate)
    test_display=$(echo "$test_name" | sed 's/^test_//' | cut -c1-50)
    printf "%-50s" "$test_display"

    for model in "${MODELS[@]}"; do
        result="${TEST_RESULTS[${model}__${test_name}]:-❓}"
        printf " | %-15s" "$result"
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
        result="${TEST_RESULTS[${model}__${test_name}]:-❓}"
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
echo "║                     RAW RESULTS                                ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

for model in "${MODELS[@]}"; do
    model_filename=$(echo "$model" | tr ':/' '_')
    log_file="$RUN_DIR/${model_filename}.log"
    echo "📄 $model: $log_file"
done

echo ""
echo "╔════════════════════════════════════════════════════════════════╗"
echo "║                       COMPLETE                                 ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""
echo "✨ Benchmark complete!"
echo "📂 All results saved to: $RUN_DIR"
echo ""

# Generate a markdown report
REPORT_FILE="$RUN_DIR/REPORT.md"

cat > "$REPORT_FILE" <<EOF
# Ollama Model Benchmark Report

**Date**: $(date)
**Models Tested**: ${#MODELS[@]}
**Tests Run**: ${#TEST_NAMES[@]}

## Comparison Table

| Test Name | $(printf "%s | " "${MODELS[@]}") |
|-----------|$(printf -- "--------|%.0s" "${MODELS[@]}")---|
EOF

for test_name in "${TEST_NAMES[@]}"; do
    test_display=$(echo "$test_name" | sed 's/^test_//')
    echo -n "| $test_display |" >> "$REPORT_FILE"
    for model in "${MODELS[@]}"; do
        result="${TEST_RESULTS[${model}__${test_name}]:-❓}"
        echo -n " $result |" >> "$REPORT_FILE"
    done
    echo "" >> "$REPORT_FILE"
done

cat >> "$REPORT_FILE" <<EOF

## Summary

EOF

for model in "${MODELS[@]}"; do
    passed=0
    failed=0

    for test_name in "${TEST_NAMES[@]}"; do
        result="${TEST_RESULTS[${model}__${test_name}]:-❓}"
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
- **Log**: [${model//[:\\/]/_}.log](./${model//[:\\/]/_}.log)

EOF
done

cat >> "$REPORT_FILE" <<EOF

## Raw Results

EOF

for model in "${MODELS[@]}"; do
    model_filename=$(echo "$model" | tr ':/' '_')
    echo "- [$model](./${model_filename}.log)" >> "$REPORT_FILE"
done

echo "📝 Markdown report: $REPORT_FILE"
echo ""
