# Phase 5: Testing Framework

## Objective

Create a comprehensive testing framework for prompt effectiveness, regression detection, and A/B testing that enables developers to validate prompt changes, measure improvements, and ensure consistent agent behavior across iterations.

## Current State Analysis

### What Exists Now
- Snapshot tests that compare output format
- E2E tests that validate basic functionality
- No prompt effectiveness measurement
- No regression detection for prompt changes
- No A/B testing capability
- No metrics on prompt performance

### Problems This Solves
1. **Blind Changes**: Can't measure if prompt changes improve behavior
2. **Regressions**: No way to detect when prompts get worse
3. **No Optimization**: Can't systematically improve prompts
4. **Lack of Confidence**: Fear of breaking things prevents improvements
5. **No Metrics**: No data on which prompts work best

## Design Specification

### Testing Framework Architecture

#### 1. Prompt Test Suite

```rust
pub struct PromptTestSuite {
    // Test cases organized by category
    test_cases: HashMap<TestCategory, Vec<PromptTestCase>>,

    // Test runner
    runner: TestRunner,

    // Metrics collector
    metrics: MetricsCollector,

    // Regression detector
    regression_detector: RegressionDetector,
}

pub struct PromptTestCase {
    // Unique identifier
    pub id: String,

    // Test metadata
    pub metadata: TestMetadata,

    // Input specification
    pub input: TestInput,

    // Expected outputs
    pub expectations: TestExpectations,

    // Evaluation criteria
    pub criteria: EvaluationCriteria,
}

pub struct TestInput {
    // User input or network event
    pub trigger: TriggerType,

    // System state
    pub state: SystemState,

    // Context
    pub context: TestContext,
}

pub enum TriggerType {
    UserInput(String),
    NetworkEvent(NetworkEvent),
    ScheduledTask(String),
}

pub struct TestExpectations {
    // Expected action type
    pub action: Option<String>,

    // Expected parameters
    pub parameters: Option<ParameterExpectations>,

    // Expected behavior patterns
    pub patterns: Vec<BehaviorPattern>,

    // Unacceptable responses
    pub negative_cases: Vec<String>,
}

pub struct EvaluationCriteria {
    // Scoring functions
    pub scorers: Vec<Box<dyn Scorer>>,

    // Pass threshold
    pub pass_threshold: f64,

    // Critical requirements
    pub must_have: Vec<Requirement>,

    // Nice to have
    pub should_have: Vec<Requirement>,
}
```

#### 2. Test Execution Engine

```rust
pub struct TestRunner {
    // LLM client for testing
    llm_client: OllamaClient,

    // Prompt builder
    prompt_builder: Arc<DynamicPromptBuilder>,

    // Execution strategy
    strategy: ExecutionStrategy,

    // Parallelism control
    parallelism: usize,
}

pub enum ExecutionStrategy {
    // Run all tests
    Full,

    // Run only critical tests
    Smoke,

    // Run tests affected by changes
    Targeted(Vec<String>),

    // A/B testing mode
    Comparison {
        baseline: PromptVersion,
        candidate: PromptVersion,
    },
}

impl TestRunner {
    pub async fn run_test(&self, test: &PromptTestCase) -> TestResult {
        // Build prompt based on test input
        let prompt = self.build_test_prompt(test).await?;

        // Query LLM
        let response = self.llm_client.query(&prompt).await?;

        // Evaluate response
        let evaluation = self.evaluate_response(&response, &test.expectations)?;

        TestResult {
            test_id: test.id.clone(),
            prompt_used: prompt,
            llm_response: response,
            evaluation,
            execution_time: elapsed,
            metadata: test.metadata.clone(),
        }
    }

    pub async fn run_suite(&self, suite: &PromptTestSuite) -> SuiteResults {
        // Run tests based on strategy
        let tests_to_run = self.select_tests(suite, &self.strategy);

        // Execute in parallel with controlled concurrency
        let results = stream::iter(tests_to_run)
            .map(|test| self.run_test(test))
            .buffer_unordered(self.parallelism)
            .collect::<Vec<_>>()
            .await;

        self.aggregate_results(results)
    }
}
```

#### 3. Evaluation System

```rust
pub trait Scorer: Send + Sync {
    fn score(&self, response: &LLMResponse, expectations: &TestExpectations) -> f64;
    fn name(&self) -> &str;
}

pub struct ActionMatchScorer;
impl Scorer for ActionMatchScorer {
    fn score(&self, response: &LLMResponse, expectations: &TestExpectations) -> f64 {
        if let Some(expected_action) = &expectations.action {
            if response.contains_action(expected_action) {
                return 1.0;
            }
        }
        0.0
    }
}

pub struct ParameterAccuracyScorer;
impl Scorer for ParameterAccuracyScorer {
    fn score(&self, response: &LLMResponse, expectations: &TestExpectations) -> f64 {
        // Score based on parameter matching
    }
}

pub struct SemanticSimilarityScorer {
    embedder: EmbeddingModel,
}
impl Scorer for SemanticSimilarityScorer {
    fn score(&self, response: &LLMResponse, expectations: &TestExpectations) -> f64 {
        // Use embeddings to measure semantic similarity
    }
}

pub struct LatencyScorer {
    target_latency: Duration,
}
impl Scorer for LatencyScorer {
    fn score(&self, response: &LLMResponse, _: &TestExpectations) -> f64 {
        // Score based on response time
    }
}
```

#### 4. Regression Detection

```rust
pub struct RegressionDetector {
    // Historical test results
    baseline: BaselineResults,

    // Detection thresholds
    thresholds: RegressionThresholds,

    // Comparison strategy
    strategy: ComparisonStrategy,
}

pub struct RegressionThresholds {
    // Acceptable degradation
    pub max_score_decrease: f64,

    // Minimum pass rate
    pub min_pass_rate: f64,

    // Maximum latency increase
    pub max_latency_increase: Duration,

    // Critical test failures
    pub critical_tests: HashSet<String>,
}

impl RegressionDetector {
    pub fn detect_regressions(
        &self,
        current: &SuiteResults,
        baseline: &SuiteResults,
    ) -> Vec<Regression> {
        let mut regressions = Vec::new();

        // Check overall pass rate
        if current.pass_rate() < baseline.pass_rate() - self.thresholds.max_score_decrease {
            regressions.push(Regression::PassRateDecreased {
                was: baseline.pass_rate(),
                now: current.pass_rate(),
            });
        }

        // Check critical tests
        for test_id in &self.thresholds.critical_tests {
            if baseline.passed(test_id) && !current.passed(test_id) {
                regressions.push(Regression::CriticalTestFailed {
                    test_id: test_id.clone(),
                });
            }
        }

        // Check individual test scores
        for (test_id, current_result) in current.results() {
            if let Some(baseline_result) = baseline.get_result(test_id) {
                if current_result.score < baseline_result.score - self.thresholds.max_score_decrease {
                    regressions.push(Regression::TestScoreDecreased {
                        test_id: test_id.clone(),
                        was: baseline_result.score,
                        now: current_result.score,
                    });
                }
            }
        }

        regressions
    }
}
```

#### 5. A/B Testing System

```rust
pub struct ABTestRunner {
    // Test configurations
    variants: Vec<PromptVariant>,

    // Statistical analyzer
    analyzer: StatisticalAnalyzer,

    // Result aggregator
    aggregator: ResultAggregator,
}

pub struct PromptVariant {
    pub id: String,
    pub name: String,
    pub prompt_config: PromptConfiguration,
    pub description: String,
}

pub struct StatisticalAnalyzer {
    // Significance level
    alpha: f64,

    // Minimum sample size
    min_samples: usize,
}

impl StatisticalAnalyzer {
    pub fn analyze_results(
        &self,
        variant_a: &[TestResult],
        variant_b: &[TestResult],
    ) -> ABTestAnalysis {
        // Calculate metrics for each variant
        let metrics_a = self.calculate_metrics(variant_a);
        let metrics_b = self.calculate_metrics(variant_b);

        // Perform statistical tests
        let significance = self.test_significance(&metrics_a, &metrics_b);

        ABTestAnalysis {
            variant_a: metrics_a,
            variant_b: metrics_b,
            winner: self.determine_winner(&metrics_a, &metrics_b, significance),
            confidence: significance.confidence,
            recommendations: self.generate_recommendations(&metrics_a, &metrics_b),
        }
    }

    fn test_significance(&self, a: &Metrics, b: &Metrics) -> SignificanceResult {
        // Use appropriate statistical test (t-test, chi-square, etc.)
    }
}
```

### Test Definition Format

#### YAML Test Definition

```yaml
# tests/prompts/user_input/create_server.yaml
test_suite: user_input_create_server
category: server_creation
priority: critical

tests:
  - id: create_http_basic
    description: "Create basic HTTP server"
    input:
      trigger:
        type: user_input
        value: "Start an HTTP server on port 8080"
      state:
        active_servers: []
    expectations:
      action: open_server
      parameters:
        port: 8080
        base_stack: http
      patterns:
        - contains: "HTTP"
        - matches_regex: "port.*8080"
    criteria:
      scorers:
        - action_match: 1.0
        - parameter_accuracy: 0.8
      pass_threshold: 0.8
      must_have:
        - action_is_open_server
        - port_is_8080

  - id: create_http_with_auth
    description: "Create HTTP server with authentication"
    input:
      trigger:
        type: user_input
        value: "Create HTTP server on 8080 with API key authentication"
      state:
        active_servers: []
    expectations:
      action: open_server
      parameters:
        port: 8080
        base_stack: http
        instruction:
          contains: ["API key", "authentication"]
    criteria:
      scorers:
        - action_match: 1.0
        - instruction_quality: 0.9
      pass_threshold: 0.85
```

### Implementation Steps

#### Step 1: Create Test Framework Core
**File**: `src/testing/prompt_test_framework.rs`

```rust
impl PromptTestSuite {
    pub fn load_from_directory(path: &Path) -> Result<Self>
    pub fn add_test(&mut self, test: PromptTestCase)
    pub fn run_all(&self, runner: &TestRunner) -> SuiteResults
    pub fn run_category(&self, category: TestCategory, runner: &TestRunner) -> SuiteResults
}
```

#### Step 2: Implement Test Runner
**File**: `src/testing/test_runner.rs`

```rust
impl TestRunner {
    pub fn new(llm_client: OllamaClient, parallelism: usize) -> Self
    pub async fn run_test(&self, test: &PromptTestCase) -> TestResult
    pub async fn run_suite(&self, suite: &PromptTestSuite) -> SuiteResults
    pub fn with_strategy(mut self, strategy: ExecutionStrategy) -> Self
}
```

#### Step 3: Build Evaluation System
**File**: `src/testing/evaluation.rs`

```rust
impl EvaluationEngine {
    pub fn evaluate(&self, response: &LLMResponse, expectations: &TestExpectations) -> Evaluation
    pub fn register_scorer(&mut self, scorer: Box<dyn Scorer>)
    pub fn calculate_score(&self, evaluation: &Evaluation) -> f64
}
```

#### Step 4: Add Regression Detection
**File**: `src/testing/regression.rs`

```rust
impl RegressionDetector {
    pub fn new(baseline: BaselineResults) -> Self
    pub fn check(&self, results: &SuiteResults) -> Vec<Regression>
    pub fn update_baseline(&mut self, results: &SuiteResults)
    pub fn generate_report(&self, regressions: &[Regression]) -> String
}
```

#### Step 5: Implement A/B Testing
**File**: `src/testing/ab_testing.rs`

```rust
impl ABTestRunner {
    pub fn new(variants: Vec<PromptVariant>) -> Self
    pub async fn run(&self, test_suite: &PromptTestSuite) -> ABTestResults
    pub fn analyze(&self, results: &ABTestResults) -> ABTestAnalysis
    pub fn recommend(&self, analysis: &ABTestAnalysis) -> Recommendation
}
```

### CLI Integration

```bash
# Run all prompt tests
netget test prompts --all

# Run specific category
netget test prompts --category user_input

# Run regression check
netget test prompts --regression --baseline v1.2.0

# Run A/B test
netget test prompts --ab --variant-a current --variant-b experimental

# Generate test report
netget test prompts --report --output report.html
```

### Testing Plan

#### Meta-Tests (Testing the Test Framework)
```rust
#[test]
fn test_scorer_accuracy() {
    let scorer = ActionMatchScorer;
    let response = mock_response_with_action("open_server");
    let expectations = TestExpectations { action: Some("open_server".into()), ..Default::default() };
    assert_eq!(scorer.score(&response, &expectations), 1.0);
}

#[test]
fn test_regression_detection() {
    let baseline = mock_baseline_results(0.9); // 90% pass rate
    let current = mock_current_results(0.7);   // 70% pass rate
    let detector = RegressionDetector::new(baseline);
    let regressions = detector.check(&current);
    assert!(!regressions.is_empty());
}
```

#### Integration Tests
1. Full test suite execution
2. Parallel test running
3. A/B comparison workflow
4. Regression detection accuracy

### Configuration

```toml
[testing]
# Test directory
test_dir = "tests/prompts/"

# Parallel test execution
parallelism = 4

# Test timeout
timeout_seconds = 30

# LLM model for testing
test_model = "qwen3-coder:30b"

[regression]
# Baseline storage
baseline_dir = "tests/baselines/"

# Regression thresholds
max_score_decrease = 0.1
min_pass_rate = 0.8

# Critical tests that must pass
critical_tests = [
    "create_http_basic",
    "handle_data_received",
]

[ab_testing]
# Statistical significance level
alpha = 0.05

# Minimum samples for significance
min_samples = 30

# Metrics to compare
compare_metrics = [
    "accuracy",
    "latency",
    "token_usage",
]
```

### Success Criteria

1. **Coverage**:
   - [ ] Test cases for all common scenarios
   - [ ] Coverage of all event types
   - [ ] Edge cases tested

2. **Reliability**:
   - [ ] Consistent results across runs
   - [ ] Low false positive rate
   - [ ] Accurate regression detection

3. **Performance**:
   - [ ] Test suite runs in < 5 minutes
   - [ ] Parallel execution working
   - [ ] Minimal LLM calls through caching

4. **Usability**:
   - [ ] Easy to add new tests
   - [ ] Clear test reports
   - [ ] Good debugging information

### Risks & Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Test flakiness | High | Deterministic prompts, retries |
| LLM cost | Medium | Caching, smaller test model |
| False positives | Medium | Tunable thresholds, multiple runs |
| Slow execution | Low | Parallelization, smart selection |

### Dependencies

- **Benefits From**: Phase 2 (Template System for test variants)
- **Independent Of**: Other phases (can test any prompt system)
- **Enhances**: All phases through validation

### Example Test Execution

```rust
// Load test suite
let suite = PromptTestSuite::load_from_directory("tests/prompts/")?;

// Configure runner
let runner = TestRunner::new(ollama_client, 4)
    .with_strategy(ExecutionStrategy::Full);

// Run tests
let results = runner.run_suite(&suite).await?;

// Check for regressions
let baseline = BaselineResults::load("v1.0.0")?;
let detector = RegressionDetector::new(baseline);
let regressions = detector.check(&results);

if !regressions.is_empty() {
    eprintln!("Regressions detected:");
    for regression in regressions {
        eprintln!("  - {}", regression);
    }
    std::process::exit(1);
}

// Generate report
let report = results.generate_report();
println!("{}", report);

// Output:
// Test Results: 45/50 passed (90%)
// Average Score: 0.87
// Average Latency: 245ms
//
// Failed Tests:
//   - create_ssh_with_key_auth (score: 0.6)
//   - handle_malformed_request (score: 0.4)
//
// Top Performing:
//   - create_http_basic (score: 1.0)
//   - handle_data_received (score: 0.95)
```

### Completion Checklist

- [ ] Test framework core implemented
- [ ] Test case format defined
- [ ] Test runner with parallelization
- [ ] Evaluation system with scorers
- [ ] Regression detection working
- [ ] A/B testing system
- [ ] Statistical analysis
- [ ] CLI integration
- [ ] Test suite for framework itself
- [ ] Example test cases
- [ ] Documentation written
- [ ] CI/CD integration