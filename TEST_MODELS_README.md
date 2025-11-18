# Model Benchmarking Script

The `test-models.sh` script runs all Ollama model tests against multiple models and generates a comprehensive comparison report.

## Usage

### Basic Usage

```bash
# Test with default models (qwen2.5-coder:7b, qwen3-coder:30b, llama3:8b)
./test-models.sh

# Test with specific models
./test-models.sh qwen2.5-coder:7b qwen3-coder:30b

# Test a single model
./test-models.sh codellama:13b
```

### Example Output

```
╔════════════════════════════════════════════════════════════════╗
║           Ollama Model Testing Benchmark                       ║
╚════════════════════════════════════════════════════════════════╝

📊 Testing 3 models
📂 Results will be saved to: ./test-results/run_20250118_143022

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
🤖 Testing model: qwen2.5-coder:7b
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
⏳ Running tests (this may take a few minutes)...

Status: ✅ PASSED
📄 Results: ./test-results/run_20250118_143022/qwen2.5-coder_7b.log

...

╔════════════════════════════════════════════════════════════════╗
║                     COMPARISON REPORT                          ║
╚════════════════════════════════════════════════════════════════╝

Test Name                                          | qwen2.5-coder:7 | qwen3-coder:30b | llama3:8b
───────────────────────────────────────────────── | ─────────────── | ─────────────── | ───────────
open_http_server                                   | ✅              | ✅              | ✅
open_tcp_server_with_port                         | ✅              | ✅              | ❌
open_server_with_instruction                       | ✅              | ✅              | ✅
dns_server_with_static_response                    | ❌              | ✅              | ❌
http_script_sum_query_params                       | ❌              | ✅              | ❌
tcp_echo_script                                    | ✅              | ✅              | ✅
...

╔════════════════════════════════════════════════════════════════╗
║                        SUMMARY                                 ║
╚════════════════════════════════════════════════════════════════╝

🤖 qwen2.5-coder:7b
   ✅ Passed: 15/20 (75%)
   ❌ Failed: 5/20

🤖 qwen3-coder:30b
   ✅ Passed: 19/20 (95%)
   ❌ Failed: 1/20

🤖 llama3:8b
   ✅ Passed: 12/20 (60%)
   ❌ Failed: 8/20
```

## Output Files

The script generates the following files in `./test-results/run_<timestamp>/`:

### Log Files

Individual test run logs for each model:
- `<model_name>.log` - Full cargo test output with all test results

Example: `qwen2.5-coder_7b.log`

### Markdown Report

`REPORT.md` - Comprehensive markdown report with:
- Comparison table (models vs tests)
- Summary statistics for each model
- Links to raw log files

## Report Structure

### Comparison Table

Shows a matrix of all tests vs all models with pass/fail indicators:

| Test Name | Model 1 | Model 2 | Model 3 |
|-----------|---------|---------|---------|
| test_1    | ✅      | ✅      | ❌      |
| test_2    | ✅      | ❌      | ✅      |

### Summary Statistics

For each model:
- Pass/fail counts
- Success rate percentage
- Link to detailed logs

### Raw Results

Links to all log files for detailed analysis.

## Use Cases

### 1. Model Evaluation

Compare different models to find the best one:

```bash
./test-models.sh qwen2.5-coder:7b qwen3-coder:30b codellama:13b deepseek-coder:6.7b
```

### 2. Model Version Comparison

Test different versions of the same model:

```bash
./test-models.sh llama3:8b llama3:70b llama3.1:8b llama3.1:70b
```

### 3. Prompt Engineering Validation

After modifying prompts, run benchmark to ensure no regressions:

```bash
# Before prompt changes
./test-models.sh qwen3-coder:30b
# Save results

# Make prompt changes...

# After prompt changes
./test-models.sh qwen3-coder:30b
# Compare results
```

### 4. Regression Testing

Ensure new model versions maintain quality:

```bash
# Test old version
./test-models.sh qwen3-coder:30b-old

# Test new version
./test-models.sh qwen3-coder:30b-new

# Compare pass rates
```

## Advanced Usage

### Custom Test Filtering

Run only specific tests by modifying the script:

```bash
# Edit the cargo test command in test-models.sh
# Change: cargo test --test ollama_model_test
# To:     cargo test --test ollama_model_test test_http
```

### Parallel Execution

Run multiple models in parallel (use with caution - high resource usage):

```bash
# Launch in background
./test-models.sh model1 &
./test-models.sh model2 &
./test-models.sh model3 &
wait
```

### CI/CD Integration

Use in continuous integration:

```yaml
# .github/workflows/model-benchmark.yml
name: Model Benchmark
on: [push]
jobs:
  benchmark:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - name: Run benchmark
        run: ./test-models.sh qwen2.5-coder:7b
      - name: Upload results
        uses: actions/upload-artifact@v2
        with:
          name: test-results
          path: test-results/
```

## Interpreting Results

### Pass Rate Guidelines

- **90-100%**: Excellent - Model handles all scenarios well
- **75-89%**: Good - Model works for most cases with minor issues
- **60-74%**: Fair - Model struggles with some complex scenarios
- **Below 60%**: Poor - Model not suitable for production use

### Common Failure Patterns

1. **Script Generation Failures**: Model generates invalid syntax
2. **Action Structure Failures**: Model doesn't follow action schema
3. **Logic Errors**: Script executes but produces wrong output
4. **Instruction Misinterpretation**: Model doesn't understand requirements

### Analyzing Failures

For failed tests, check the log files for details:

```bash
# View failures for a specific model
grep "FAILED" ./test-results/run_*/qwen2.5-coder_7b.log

# View detailed error messages
grep -A 10 "❌" ./test-results/run_*/qwen2.5-coder_7b.log
```

## Troubleshooting

### Script Hangs

If the script hangs during testing:
- Check Ollama is running: `curl http://localhost:11434/api/tags`
- Verify model is available: `ollama list`
- Increase timeout in cargo test command

### Memory Issues

Large models may require more memory:
- Reduce number of concurrent models
- Use smaller model variants
- Increase system swap space

### Incomplete Results

If results are missing:
- Check log files for errors
- Verify all models completed
- Re-run with `--nocapture` for verbose output

## Example Workflow

### Complete Model Evaluation

```bash
# 1. Pull candidate models
ollama pull qwen2.5-coder:7b
ollama pull qwen3-coder:30b
ollama pull codellama:13b

# 2. Run benchmark
./test-models.sh qwen2.5-coder:7b qwen3-coder:30b codellama:13b

# 3. Review comparison table (stdout)
# Look for highest pass rate

# 4. Review detailed failures
cd test-results/run_<timestamp>
cat REPORT.md

# 5. Investigate specific failures
grep "❌ Script" qwen2.5-coder_7b.log

# 6. Make decision based on results
# - Choose model with highest pass rate
# - Consider model size vs performance tradeoff
# - Verify critical tests pass
```

## Tips

### Speed Up Testing

1. **Use smaller models first** for quick iteration
2. **Test subset of models** before full benchmark
3. **Run in parallel** (if resources allow)
4. **Use SSD** for faster log writes

### Improve Accuracy

1. **Run multiple times** to account for LLM non-determinism
2. **Use larger context window** models for complex tests
3. **Adjust temperature** via Ollama settings for consistency
4. **Test with latest model versions**

### Better Comparisons

1. **Test same model family** (e.g., qwen 7b vs 30b)
2. **Include baseline model** for reference
3. **Document environment** (Ollama version, system specs)
4. **Save historical results** for trend analysis

## See Also

- `tests/ollama_model_test.rs` - Test implementations
- `tests/OLLAMA_MODEL_TESTING.md` - Testing framework documentation
- `tests/helpers/ollama_test_builder.rs` - Test builder implementation
