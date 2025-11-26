#!/bin/bash
# Ollama Model Benchmarking Script
#
# Runs all Ollama model tests against multiple models and generates
# a comprehensive comparison report.
#
# Usage:
#   ./test-models.sh [OPTIONS] [model1] [model2] [model3] ...
#
# Options:
#   -t, --test TEST_NAME    Run specific test(s) only (can be used multiple times)
#                          If not specified, runs all discovered tests
#
# Examples:
#   # Run all tests with default models
#   ./test-models.sh
#
#   # Run all tests with specific models
#   ./test-models.sh qwen2.5-coder:7b qwen3-coder:30b llama3:8b
#
#   # Run specific test with default models
#   ./test-models.sh --test test_basic_prompt
#
#   # Run multiple specific tests with custom models
#   ./test-models.sh -t test_basic_prompt -t test_json_mode qwen2.5-coder:7b
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

# Parse options
SPECIFIED_TESTS=()
MODELS=()

while [ $# -gt 0 ]; do
    case "$1" in
        -t|--test)
            if [ $# -lt 2 ]; then
                echo "Error: --test requires a test name argument"
                exit 1
            fi
            SPECIFIED_TESTS+=("$2")
            shift 2
            ;;
        -h|--help)
            # Print usage and exit
            head -n 30 "$0" | grep "^#" | sed 's/^# \?//'
            exit 0
            ;;
        *)
            # Assume remaining args are models
            MODELS+=("$1")
            shift
            ;;
    esac
done

# Require explicit model selection
if [ ${#MODELS[@]} -eq 0 ]; then
    echo "❌ Error: No models specified"
    echo ""
    echo "You must explicitly specify which model(s) to test."
    echo ""
    echo "Available models (commonly used):"
    echo "  • qwen2.5-coder:7b"
    echo "  • qwen3-coder:30b"
    echo "  • llama3:8b"
    echo ""
    echo "Usage:"
    echo "  $0 <model1> [model2] [model3] ..."
    echo ""
    echo "Examples:"
    echo "  $0 qwen3-coder:30b"
    echo "  $0 qwen2.5-coder:7b qwen3-coder:30b"
    echo ""
    echo "To see all available models on your system:"
    echo "  ollama list"
    exit 1
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
if [ ${#SPECIFIED_TESTS[@]} -gt 0 ]; then
    echo "🎯 Will run ${#SPECIFIED_TESTS[@]} specific test(s) (after compilation)"
    echo ""
fi

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
ALL_TEST_NAMES=()
while IFS= read -r line; do
    if [[ "$line" =~ ^([a-zA-Z0-9_]+):\ test$ ]]; then
        test_name="${BASH_REMATCH[1]}"
        ALL_TEST_NAMES+=("$test_name")
    fi
done <<< "$TEST_LIST_OUTPUT"

echo "Found ${#ALL_TEST_NAMES[@]} tests"

# Filter tests if specific tests were requested
if [ ${#SPECIFIED_TESTS[@]} -gt 0 ]; then
    TEST_NAMES=()
    for specified_test in "${SPECIFIED_TESTS[@]}"; do
        # Check if specified test exists in discovered tests
        found=false
        for discovered_test in "${ALL_TEST_NAMES[@]}"; do
            if [ "$discovered_test" = "$specified_test" ]; then
                TEST_NAMES+=("$specified_test")
                found=true
                break
            fi
        done

        if [ "$found" = false ]; then
            echo "⚠️  Warning: Test '$specified_test' not found in discovered tests"
        fi
    done

    if [ ${#TEST_NAMES[@]} -eq 0 ]; then
        echo "❌ Error: None of the specified tests were found"
        echo ""
        echo "Available tests:"
        for test in "${ALL_TEST_NAMES[@]}"; do
            echo "  - $test"
        done
        exit 1
    fi

    echo "ℹ️  Running ${#TEST_NAMES[@]} specified test(s):"
    for test in "${TEST_NAMES[@]}"; do
        echo "   • $test"
    done
else
    TEST_NAMES=("${ALL_TEST_NAMES[@]}")
    echo "ℹ️  Running all ${#TEST_NAMES[@]} tests"
fi

echo ""
echo "Running tests sequentially for accurate benchmarking..."
echo "Testing same test across all models for direct comparison"
echo ""

# Individual log files for each test/model combination
LOGS_DIR="$RUN_DIR/logs"
mkdir -p "$LOGS_DIR"
echo "Log directory: $LOGS_DIR"
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

    # Print test name (no trailing separator - matches header format)
    printf "%-40s" "$test_display"

    # Flush output to show the row immediately
    printf ""

    # Run this test for each model and print results in columns
    for model in "${MODELS[@]}"; do
        # Sanitize names for filename (replace : and / with _)
        model_safe=$(echo "$model" | tr ':/' '_')
        test_safe=$(echo "$test_name" | tr ':/' '_')

        # Create individual log file: {test_name}_{model}.log
        LOG_FILE="$LOGS_DIR/${test_safe}_${model_safe}.log"

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

        # Print footer to log file
        echo "" >> "$LOG_FILE"
        echo "───────────────────────────────────────────────────────────────" >> "$LOG_FILE"
        echo "End: $test_name ($model) - Exit code: $test_exit_code" >> "$LOG_FILE"
        echo "───────────────────────────────────────────────────────────────" >> "$LOG_FILE"
        echo "" >> "$LOG_FILE"

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
echo "║                     DETAILED LOGS                              ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""
echo "📁 Individual test logs: $LOGS_DIR"
echo ""

# Count total log files and failures across all logs
log_count=$(ls -1 "$LOGS_DIR"/*.log 2>/dev/null | wc -l | tr -d ' ')
failure_count=$(grep -h "Error: Test failed" "$LOGS_DIR"/*.log 2>/dev/null | wc -l | tr -d ' ')
echo "   Total log files: $log_count"
echo "   Total assertion failures: $failure_count"
echo ""

echo "Each log file contains:"
echo "  • Header with model and test name"
echo "  • Full cargo test output for that specific test"
echo "  • LLM responses"
echo "  • Assertion failures with expected vs actual values"
echo "  • Stack traces for panics"
echo ""
echo "Log naming format: {test_name}_{model}.log"
echo ""
echo "╔════════════════════════════════════════════════════════════════╗"
echo "║                       COMPLETE                                 ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""
echo "✨ Benchmark complete!"
echo "📊 Results table: See above"
echo "📂 All files saved to: $RUN_DIR"
echo "📁 Individual logs: $LOGS_DIR"
echo "📝 HTML report: $RUN_DIR/report.html"
echo ""

# Generate an HTML report
REPORT_FILE="$RUN_DIR/report.html"

# Count total failures across all log files
log_count=$(ls -1 "$LOGS_DIR"/*.log 2>/dev/null | wc -l | tr -d ' ')
failure_count=$(grep -h "Error: Test failed" "$LOGS_DIR"/*.log 2>/dev/null | wc -l | tr -d ' ')

# Start HTML document
cat > "$REPORT_FILE" <<'EOF'
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Ollama Model Benchmark Report</title>
    <style>
        * { box-sizing: border-box; }
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Helvetica, Arial, sans-serif;
            line-height: 1.6;
            color: #24292e;
            max-width: 1400px;
            margin: 0 auto;
            padding: 20px;
            background: #f6f8fa;
        }
        .header {
            background: white;
            padding: 30px;
            border-radius: 8px;
            box-shadow: 0 1px 3px rgba(0,0,0,0.1);
            margin-bottom: 20px;
        }
        h1 { margin: 0 0 10px 0; color: #0366d6; }
        .meta { color: #586069; font-size: 14px; }
        .meta span { margin-right: 20px; }

        .toc {
            background: white;
            padding: 20px 30px;
            border-radius: 8px;
            box-shadow: 0 1px 3px rgba(0,0,0,0.1);
            margin-bottom: 20px;
        }
        .toc h2 { margin-top: 0; color: #24292e; font-size: 20px; }
        .toc ul { list-style: none; padding: 0; margin: 0; }
        .toc li { padding: 8px 0; border-bottom: 1px solid #e1e4e8; }
        .toc li:last-child { border-bottom: none; }
        .toc a { color: #0366d6; text-decoration: none; }
        .toc a:hover { text-decoration: underline; }

        .test-section {
            background: white;
            padding: 30px;
            border-radius: 8px;
            box-shadow: 0 1px 3px rgba(0,0,0,0.1);
            margin-bottom: 20px;
        }
        .test-section h2 {
            margin-top: 0;
            color: #24292e;
            border-bottom: 2px solid #e1e4e8;
            padding-bottom: 10px;
        }

        .model-result {
            border: 1px solid #e1e4e8;
            border-radius: 6px;
            padding: 20px;
            margin: 15px 0;
            background: #fafbfc;
        }
        .model-result h3 {
            margin: 0 0 15px 0;
            display: flex;
            align-items: center;
            gap: 10px;
        }
        .status {
            display: inline-block;
            padding: 4px 12px;
            border-radius: 12px;
            font-size: 12px;
            font-weight: 600;
        }
        .status.pass { background: #dcffe4; color: #22863a; }
        .status.fail { background: #ffdce0; color: #d73a49; }
        .status.timeout { background: #fff5b1; color: #735c0f; }

        .failure-details {
            background: #fff5f5;
            border-left: 4px solid #d73a49;
            padding: 15px;
            margin: 15px 0;
            border-radius: 4px;
        }
        .failure-details h4 {
            margin: 0 0 10px 0;
            color: #d73a49;
            font-size: 14px;
        }
        .failure-details pre {
            background: white;
            padding: 10px;
            border-radius: 3px;
            overflow-x: auto;
            font-size: 12px;
            margin: 5px 0;
        }
        .expected-actual {
            display: grid;
            grid-template-columns: 1fr 1fr;
            gap: 10px;
            margin-top: 10px;
        }
        .expected-actual > div {
            background: white;
            padding: 10px;
            border-radius: 3px;
        }
        .expected-actual h5 {
            margin: 0 0 8px 0;
            font-size: 12px;
            text-transform: uppercase;
            color: #586069;
        }

        details {
            margin: 15px 0;
            border: 1px solid #e1e4e8;
            border-radius: 6px;
            padding: 10px;
            background: white;
        }
        summary {
            cursor: pointer;
            font-weight: 600;
            padding: 5px;
            user-select: none;
            color: #0366d6;
        }
        summary:hover { text-decoration: underline; }
        .log-content {
            margin-top: 10px;
            padding: 15px;
            background: #f6f8fa;
            border-radius: 4px;
            max-height: 400px;
            overflow: auto;
        }
        .log-content pre {
            margin: 0;
            font-size: 11px;
            line-height: 1.4;
            white-space: pre-wrap;
            word-wrap: break-word;
        }

        .back-to-top {
            position: fixed;
            bottom: 20px;
            right: 20px;
            background: #0366d6;
            color: white;
            padding: 10px 20px;
            border-radius: 6px;
            text-decoration: none;
            box-shadow: 0 2px 8px rgba(0,0,0,0.2);
        }
        .back-to-top:hover { background: #0256c7; }
    </style>
</head>
<body>
EOF

# Add header
cat >> "$REPORT_FILE" <<EOF
    <div class="header">
        <h1>🤖 Ollama Model Benchmark Report</h1>
        <div class="meta">
            <span>📅 <strong>Date:</strong> $(date)</span>
            <span>🔢 <strong>Models:</strong> ${#MODELS[@]}</span>
            <span>🧪 <strong>Tests:</strong> ${#TEST_NAMES[@]}</span>
            <span>📁 <strong>Log Files:</strong> $log_count</span>
            <span>❌ <strong>Failures:</strong> $failure_count</span>
        </div>
    </div>
EOF

# Add table of contents
cat >> "$REPORT_FILE" <<'EOF'
    <div class="toc">
        <h2>📑 Table of Contents</h2>
        <ul>
EOF

for test_name in "${TEST_NAMES[@]}"; do
    test_display=$(echo "$test_name" | sed 's/^test_//')
    test_anchor=$(echo "$test_name" | tr '[:upper:]' '[:lower:]' | tr '_' '-')

    # Count pass/fail across all models for this test
    passed=0
    failed=0
    for model in "${MODELS[@]}"; do
        result=$(get_result "$model" "$test_name")
        if [ "$result" = "✅" ]; then
            ((passed++)) || true
        else
            ((failed++)) || true
        fi
    done

    # Determine overall status
    if [ $failed -eq 0 ]; then
        status_badge="<span class=\"status pass\">✅ PASS</span>"
    elif [ $passed -eq 0 ]; then
        status_badge="<span class=\"status fail\">❌ FAIL</span>"
    else
        status_badge="<span class=\"status timeout\">⚠️ MIXED</span>"
    fi

    echo "            <li><a href=\"#test-$test_anchor\">$status_badge $test_display</a></li>" >> "$REPORT_FILE"
done

cat >> "$REPORT_FILE" <<'EOF'
        </ul>
    </div>
EOF

# Function to extract failure details from log
extract_failure_details() {
    local log_file="$1"
    local temp_file=$(mktemp)

    grep -B 3 -A 10 -i "failures:|=== LLM RESPONSE ===|assertion\|expected\|panicked at" "$log_file" 2>/dev/null | \
        grep -v "^--$" | \
        head -50 > "$temp_file"

    cat "$temp_file"
    rm -f "$temp_file"
}

# Add test sections
for test_name in "${TEST_NAMES[@]}"; do
    test_display=$(echo "$test_name" | sed 's/^test_//')
    test_anchor=$(echo "$test_name" | tr '[:upper:]' '[:lower:]' | tr '_' '-')

    # Count pass/fail across all models for this test
    passed=0
    failed=0
    for model in "${MODELS[@]}"; do
        result=$(get_result "$model" "$test_name")
        if [ "$result" = "✅" ]; then
            ((passed++)) || true
        else
            ((failed++)) || true
        fi
    done

    # Determine overall status for header
    if [ $failed -eq 0 ]; then
        header_status="✅ PASS"
    elif [ $passed -eq 0 ]; then
        header_status="❌ FAIL"
    else
        header_status="⚠️ MIXED"
    fi

    cat >> "$REPORT_FILE" <<EOF
    <div class="test-section" id="test-$test_anchor">
        <h2>$header_status - $test_display</h2>
EOF

    # Add results for each model
    for model in "${MODELS[@]}"; do
        result=$(get_result "$model" "$test_name")

        # Determine status class
        status_class="timeout"
        status_text="Timeout"
        if [ "$result" = "✅" ]; then
            status_class="pass"
            status_text="Passed"
        elif [ "$result" = "❌" ]; then
            status_class="fail"
            status_text="Failed"
        fi

        # Get log file
        model_safe=$(echo "$model" | tr ':/' '_')
        test_safe=$(echo "$test_name" | tr ':/' '_')
        log_file="$LOGS_DIR/${test_safe}_${model_safe}.log"

        cat >> "$REPORT_FILE" <<EOF
        <div class="model-result">
            <h3>
                <span>$model</span>
                <span class="status $status_class">$status_text</span>
            </h3>
EOF

        # If failed, extract and show failure details
        if [ "$result" = "❌" ] && [ -f "$log_file" ]; then
            failure_details=$(extract_failure_details "$log_file")

            if [ -n "$failure_details" ]; then
                # HTML escape the content
                failure_details_escaped=$(echo "$failure_details" | sed 's/&/\&amp;/g; s/</\&lt;/g; s/>/\&gt;/g; s/"/\&quot;/g')

                cat >> "$REPORT_FILE" <<EOF
            <div class="failure-details">
                <h4>❌ Failure Details</h4>
                <pre>$failure_details_escaped</pre>
            </div>
EOF
            fi
        fi

        # Add collapsible full log
        if [ -f "$log_file" ]; then
            log_content=$(cat "$log_file" | sed 's/&/\&amp;/g; s/</\&lt;/g; s/>/\&gt;/g')

            cat >> "$REPORT_FILE" <<EOF
            <details>
                <summary>📋 View Full Log</summary>
                <div class="log-content">
                    <pre>$log_content</pre>
                </div>
            </details>
EOF
        fi

        cat >> "$REPORT_FILE" <<'EOF'
        </div>
EOF
    done

    cat >> "$REPORT_FILE" <<'EOF'
    </div>
EOF
done

# Close HTML
cat >> "$REPORT_FILE" <<'EOF'
    <a href="#" class="back-to-top">↑ Back to Top</a>

    <script>
        // Scroll log content to bottom when details are opened
        document.addEventListener('DOMContentLoaded', function() {
            const detailsElements = document.querySelectorAll('details');

            detailsElements.forEach(function(details) {
                details.addEventListener('toggle', function() {
                    if (this.open) {
                        const logContent = this.querySelector('.log-content');
                        if (logContent) {
                            // Small delay to ensure content is rendered
                            setTimeout(function() {
                                logContent.scrollTop = logContent.scrollHeight;
                            }, 10);
                        }
                    }
                });
            });
        });
    </script>
</body>
</html>
EOF
