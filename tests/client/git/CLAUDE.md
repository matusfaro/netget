# Git Client E2E Test Documentation

## Overview

End-to-end tests for the Git client protocol implementation. Tests LLM-controlled Git operations including clone, fetch, branch listing, log viewing, and status checking.

## Test Strategy

### Approach

**Black-box testing**: Tests use real Git operations with public repositories to validate LLM integration and Git library functionality.

**Test Categories:**
1. **Clone Operations**: Test repository cloning from public GitHub repos
2. **Inspection Operations**: Test log, status, and branch listing
3. **Update Operations**: Test fetch (future: pull, push)

### Test Infrastructure

**Test Setup:**
- Use `tempfile::TempDir` for temporary clone directories
- Use public GitHub repositories (no authentication required)
- Clone small repositories to minimize test runtime
- Use `git2` directly for test setup (pre-cloning repos for inspection tests)

**Repository Selection:**
- **Primary**: `https://github.com/rust-lang/rustlings.git` (small, stable, well-known)
- **Alternative**: Any small public repository

**Cleanup:**
- Temporary directories automatically cleaned up by `TempDir::drop()`
- No manual cleanup required

## LLM Call Budget

**Target**: < 10 LLM calls total across all tests

**Breakdown:**
- `test_git_clone`: 2-3 LLM calls (initial + clone result)
- `test_git_list_branches`: 2-3 LLM calls (initial + result)
- `test_git_log`: 2-3 LLM calls (initial + result)
- `test_git_status`: 2-3 LLM calls (initial + result)

**Total**: ~8-12 LLM calls (close to budget, may need optimization)

**Optimization Strategies:**
1. Mark tests as `#[ignore]` to run selectively
2. Combine multiple operations in single test (e.g., clone + log + status)
3. Use `scripting` mode for deterministic operations (future)

## Expected Runtime

**Per Test:**
- `test_git_clone`: 20-40 seconds (depends on network speed and repo size)
- `test_git_list_branches`: 5-10 seconds (quick inspection)
- `test_git_log`: 5-10 seconds (quick inspection)
- `test_git_status`: 5-10 seconds (quick inspection)

**Total Suite**: ~40-70 seconds (if run sequentially)

**Network Dependency**: Tests require internet access to clone from GitHub

## Test Implementation

### Current Tests

1. **test_git_clone**
   - **Purpose**: Validate LLM can clone a repository
   - **Steps**:
     1. Create temp directory
     2. Set LLM instruction to clone rustlings repo
     3. Connect Git client (triggers LLM)
     4. Wait for clone to complete
     5. Verify `.git` directory exists
   - **Assertions**: Clone path exists, `.git` directory exists
   - **LLM Calls**: 2-3 (initial event, clone complete)

2. **test_git_list_branches**
   - **Purpose**: Validate LLM can list branches
   - **Steps**:
     1. Pre-clone repo using git2
     2. Set LLM instruction to list branches
     3. Connect Git client
     4. Wait for operation
   - **Assertions**: Operation completes without error
   - **LLM Calls**: 2-3

3. **test_git_log**
   - **Purpose**: Validate LLM can retrieve commit history
   - **Steps**:
     1. Pre-clone repo using git2
     2. Set LLM instruction to show last 5 commits
     3. Connect Git client
     4. Wait for operation
   - **Assertions**: Operation completes without error
   - **LLM Calls**: 2-3

4. **test_git_status**
   - **Purpose**: Validate LLM can check repo status
   - **Steps**:
     1. Pre-clone repo using git2
     2. Set LLM instruction to check status
     3. Connect Git client
     4. Wait for operation
   - **Assertions**: Operation completes without error
   - **LLM Calls**: 2-3

### Future Tests

1. **test_git_fetch**
   - Test fetching updates from remote
   - Requires pre-cloned repo

2. **test_git_checkout**
   - Test checking out different branches
   - Requires pre-cloned repo with multiple branches

3. **test_git_authenticated_clone**
   - Test cloning private repo with credentials
   - Requires test account and PAT
   - Security: Use environment variables for credentials

## Known Issues

1. **Network Flakiness**: Tests may fail if GitHub is unreachable or slow
   - **Mitigation**: Use `#[ignore]` and run selectively
   - **Future**: Cache cloned repos or use local Git server

2. **LLM Dependency**: Tests require Ollama to be running
   - **Skip Condition**: Tests are `#[ignore]` by default
   - **CI**: Requires Ollama setup in CI environment

3. **Clone Time Variability**: Clone time depends on network speed
   - **Current**: Fixed 30-second timeout
   - **Future**: Dynamic timeout based on repo size

4. **Tempfile Cleanup**: On Windows, cleanup may fail if handles are still open
   - **Rare**: Usually not an issue
   - **Workaround**: Manual cleanup if needed

## Running Tests

### Run All Git Client Tests

```bash
./cargo-isolated.sh test --no-default-features --features git --test client::git::e2e_test -- --ignored
```

### Run Specific Test

```bash
./cargo-isolated.sh test --no-default-features --features git --test client::git::e2e_test test_git_clone -- --ignored
```

### Prerequisites

1. **Ollama Running**: `ollama serve` (with qwen3-coder:30b model)
2. **Internet Access**: For cloning from GitHub
3. **libgit2**: System library installed
   - Ubuntu/Debian: `apt-get install libgit2-dev`
   - macOS: `brew install libgit2`

## Performance Optimization

### Current Bottlenecks

1. **Network I/O**: Cloning from GitHub takes 10-30 seconds
2. **LLM Calls**: Each call takes 2-5 seconds
3. **Sequential Execution**: Tests run one at a time

### Future Improvements

1. **Local Git Server**: Run local Git server for faster clones
2. **Scripting Mode**: Use scripting for deterministic operations (no LLM)
3. **Parallel Tests**: Run independent tests concurrently
4. **Cached Clones**: Reuse cloned repos across tests

## Test Validation

### What We Test

✅ Git clone from public repository
✅ List branches (local and remote)
✅ Get commit log with limit
✅ Check repository status
✅ LLM integration (event handling, action execution)

### What We Don't Test (Yet)

❌ Authenticated operations (private repos)
❌ Fetch/pull operations
❌ Push operations
❌ Checkout operations
❌ Error handling (invalid URLs, auth failures)
❌ SSH authentication

## Security Considerations

1. **Public Repos Only**: Tests use public repositories (no credentials)
2. **Temporary Directories**: All clones in temp dirs (auto-cleaned)
3. **No Network Listening**: Git client doesn't open ports
4. **Future**: Use environment variables for test credentials (not hardcoded)

## Dependencies

### Test-Only Dependencies

```toml
[dev-dependencies]
tempfile = "3.x"  # For temporary directories
```

### Required for Tests

- `git2` (already in main dependencies as optional)
- `tokio` (already in main dependencies)
- Public internet access

## CI/CD Considerations

**CI Setup:**
1. Install libgit2 system library
2. Install Ollama and pull model
3. Enable `--ignored` tests (or skip Git client tests)

**GitHub Actions Example:**
```yaml
- name: Install libgit2
  run: sudo apt-get install -y libgit2-dev

- name: Run Git client tests
  run: ./cargo-isolated.sh test --no-default-features --features git --test client::git::e2e_test -- --ignored
```

## References

- [git2-rs Documentation](https://docs.rs/git2/)
- [GitHub API Rate Limits](https://docs.github.com/en/rest/overview/resources-in-the-rest-api#rate-limiting)
- [NetGet Test Infrastructure](../../../TEST_INFRASTRUCTURE_FIXES.md)
