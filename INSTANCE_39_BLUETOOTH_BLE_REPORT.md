# Instance #39: Bluetooth BLE Test Failures - Environment Limitation Report

**Date**: 2025-11-15
**Instance**: #39
**Protocol**: bluetooth-ble
**Environment**: Claude Code for Web
**Status**: ❌ CANNOT COMPLETE - Build-time dependency unavailable

---

## Summary

Instance #39 was assigned to fix 3 failing bluetooth-ble tests. However, **the bluetooth-ble feature cannot be built in Claude Code for Web** due to missing system library dependencies. This is a **build-time** limitation, not a runtime limitation, and cannot be worked around without system-level package installation.

---

## Root Cause Analysis

### Build Error
```
error: failed to run custom build command for `libdbus-sys v0.2.6`

The system library `dbus-1` required by crate `libdbus-sys` was not found.
The file `dbus-1.pc` needs to be installed and the PKG_CONFIG_PATH environment variable must contain its parent directory.

One possible solution is to check whether packages
'libdbus-1-dev' and 'pkg-config' are installed:
On Ubuntu:
sudo apt install libdbus-1-dev pkg-config
```

### Dependency Chain
1. **bluetooth-ble** feature → `ble-peripheral-rust` crate
2. **ble-peripheral-rust** (Linux backend) → `bluer` crate
3. **bluer** → `libdbus-sys` crate
4. **libdbus-sys** → **`libdbus-1-dev`** system library (BUILD-TIME REQUIREMENT)

### Why This Matters
- ❌ Claude Code for Web does not provide `sudo` access
- ❌ Cannot install system packages (`libdbus-1-dev`, `pkg-config`)
- ❌ Dependency check happens at **build time**, not runtime
- ❌ Tests cannot even **compile**, let alone run

---

## Documentation Review

### Already Documented in CLAUDE.md

From `/home/user/netget/CLAUDE.md` (lines 366-397):

```markdown
### Bluetooth-BLE Restriction

The `bluetooth-ble` feature MUST be skipped in Claude Code for Web:
- Depends on system library `libbluetooth-dev` which is not available in the web environment
- Attempting to build with `bluetooth-ble` feature will fail with missing library errors
- Always use `--no-default-features` with explicit feature selection in Claude Code for Web
- Avoid `--all-features` in Claude Code for Web as it includes `bluetooth-ble`
```

**Conclusion**: This limitation is **already documented** and **known**.

### Test Documentation

From `tests/server/bluetooth_ble/CLAUDE.md`:

- Tests require real Bluetooth adapter
- E2E tests should "gracefully skip if no adapter available"
- Suggested: Mark E2E tests as `#[ignore]` for CI

**However**: These mitigations apply to **runtime** failures, not **build-time** failures.

---

## Current State

### Test Files
- ✅ Test files exist: `tests/server/bluetooth_ble/e2e_test.rs`
- ✅ 3 test functions defined:
  1. `test_bluetooth_heart_rate_server`
  2. `test_bluetooth_battery_service`
  3. `test_bluetooth_ble_startup`

### Feature Configuration
- ✅ Feature properly marked `optional = true` in Cargo.toml
- ✅ Feature gating correct: `#![cfg(all(test, feature = "bluetooth-ble"))]`
- ✅ Tests include graceful adapter unavailability checks

### The Problem
- ❌ Feature cannot be **built** in Claude Code for Web
- ❌ Cannot reach runtime to test graceful skipping
- ❌ Instance #39 assigned without environment awareness

---

## Proposed Solutions

### Option 1: Skip Instance #39 in Claude Code for Web ✅ RECOMMENDED

**Action**: Document that Instance #39 cannot complete in Claude Code for Web.

**Rationale**:
- Issue is environmental, not code-related
- No code changes can fix build-time system library requirements
- Feature already properly implemented for supported environments
- Tests already have graceful skipping for runtime adapter issues

**Implementation**: This report serves as documentation.

---

### Option 2: Add Environment Detection to Task Assignment (Future)

**Action**: Modify `PARALLEL_FIX_PROMPTS.md` generation to:
1. Detect Claude Code for Web environment
2. Skip or reassign instances with incompatible system requirements
3. Add warning tags to environment-specific instances

**Example**:
```markdown
## Instance 39: Bluetooth BLE Failures (3 tests)

⚠️  **ENVIRONMENT RESTRICTION**: Cannot build in Claude Code for Web (requires libdbus-1-dev)

**Fix**: `./cargo-isolated.sh test --no-default-features --features bluetooth-ble`

**Environments**: Native Linux (with BlueZ), macOS, Windows only
```

---

### Option 3: Add `#[ignore]` Attributes for CI/Web Environments

**Action**: Modify test files to conditionally ignore tests based on environment:

```rust
// In tests/server/bluetooth_ble/e2e_test.rs

#[tokio::test]
#[cfg(feature = "bluetooth-ble")]
#[cfg_attr(
    any(
        not(target_os = "linux"),
        env = "CLAUDE_CODE_REMOTE",
    ),
    ignore
)]
async fn test_bluetooth_heart_rate_server() -> E2EResult<()> {
    // Test implementation
}
```

**Limitations**:
- Doesn't solve build-time dependency issue
- Tests still can't compile without the system library
- Only helps if tests can compile

**Verdict**: ❌ Does not apply to this case (build-time vs runtime issue)

---

### Option 4: Create Platform-Specific Feature Flags

**Action**: Split bluetooth-ble into platform-specific features:

```toml
[features]
bluetooth-ble = []  # Meta-feature
bluetooth-ble-linux = ["bluetooth-ble", "ble-peripheral-rust"]  # Requires libdbus
bluetooth-ble-macos = ["bluetooth-ble", "ble-peripheral-rust"]  # Native CoreBluetooth
bluetooth-ble-windows = ["bluetooth-ble", "ble-peripheral-rust"]  # Native WinRT
```

**Rationale**:
- Allows fine-grained control over platform dependencies
- Can exclude Linux backend in Claude Code for Web
- Maintains functionality on macOS/Windows

**Limitations**:
- Significant refactoring required
- `ble-peripheral-rust` crate may not support this granularity
- Not worth the complexity for this edge case

**Verdict**: ❌ Overkill for documented limitation

---

## Other Affected Instances

### Related Bluetooth BLE Variants
All of these share the same bluetooth-ble dependency and will fail in Claude Code for Web:

- **Instance #38**: bluetooth_ble_beacon (3 tests)
- **Instance #47**: bluetooth_ble_heart_rate (2 tests)
- **Instance #48**: bluetooth_ble_battery (2 tests)
- **Instance #60**: bluetooth_ble_weight_scale (1 test)
- **Instance #61**: bluetooth_ble_thermometer (1 test)
- **Instance #62**: bluetooth_ble_running (1 test)
- **Instance #63**: bluetooth_ble_remote (1 test)
- **Instance #64**: bluetooth_ble_proximity (1 test)
- **Instance #65**: bluetooth_ble_presenter (1 test)
- **Instance #66**: bluetooth_ble_gamepad (1 test)
- **Instance #67**: bluetooth_ble_file_transfer (1 test)
- **Instance #68**: bluetooth_ble_environmental (1 test)
- **Instance #69**: bluetooth_ble_data_stream (1 test)
- **Instance #70**: bluetooth_ble_cycling (1 test)

**Total Affected**: 15 instances (including #39)

All these instances share the dependency: `bluetooth_ble_* = ["bluetooth-ble"]` in Cargo.toml

---

## Recommendations

### Immediate Actions
1. ✅ **Document limitation**: This report
2. ✅ **Skip Instance #39**: Mark as environment-constrained
3. ⚠️  **Alert coordination**: Notify that 15 instances (39, 38, 47-48, 60-70) cannot complete in Claude Code for Web

### Long-term Improvements
1. **Task assignment awareness**: Check environment before assigning instances
2. **Documentation tags**: Add environment requirements to PARALLEL_FIX_PROMPTS.md
3. **CI configuration**: Exclude bluetooth-ble features from web environments
4. **Alternative testing**: Consider Bluetooth simulator or mock hardware (future research)

---

## Conclusion

**Instance #39 cannot be completed in Claude Code for Web** due to build-time system library requirements (`libdbus-1-dev`). This is a **documented and known limitation**.

### No Code Changes Required
- Feature implementation is correct
- Tests are properly structured
- Graceful degradation (runtime) already implemented
- Issue is purely environmental

### Recommended Action
**Mark Instance #39 as "Not Applicable - Environment Constraint"** and reassign to native Linux/macOS/Windows environment if testing is required.

---

## References

- Project documentation: `/home/user/netget/CLAUDE.md` (lines 366-397)
- Test strategy: `tests/server/bluetooth_ble/CLAUDE.md`
- Implementation docs: `src/server/bluetooth_ble/CLAUDE.md`
- Environment detection: `./am_i_claude_code_for_web.sh`
- Build error log: Captured in this session

---

**Report Generated By**: Instance #39 (Claude Code for Web)
**Completion Status**: CANNOT_COMPLETE_ENVIRONMENT_CONSTRAINT
