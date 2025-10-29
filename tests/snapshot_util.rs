//! Unified snapshot testing utility
//!
//! Provides consistent snapshot testing across all test suites.
//! Uses .snap files for expected output and .actual.snap for mismatches.

use std::fs;

/// Assert that a value matches a snapshot file
///
/// # Snapshot File Naming
/// - `{test_name}.snap` - Expected snapshot
/// - `{test_name}.actual.snap` - Actual output when test fails (gitignored)
///
/// # Example
/// ```no_run
/// assert_snapshot("my_test", "snapshots", "test output");
/// ```
///
/// If the snapshot doesn't match:
/// 1. Creates `{test_name}.actual.snap` with actual output
/// 2. Prints diff instructions
/// 3. Panics with helpful message
pub fn assert_snapshot(test_name: &str, snapshot_dir: &str, actual: &str) {
    let snapshot_path = format!("{}/{}.snap", snapshot_dir, test_name);
    let actual_path = format!("{}/{}.actual.snap", snapshot_dir, test_name);

    // Ensure snapshot directory exists
    fs::create_dir_all(snapshot_dir).ok();

    // Read expected snapshot
    let expected = match fs::read_to_string(&snapshot_path) {
        Ok(content) => content,
        Err(_) => {
            // Snapshot doesn't exist, create it
            fs::write(&snapshot_path, actual).expect("Failed to write initial snapshot");
            println!("✓ Created initial snapshot: {}", snapshot_path);
            return;
        }
    };

    // Compare actual vs expected
    if actual != expected {
        // Write actual output for comparison
        fs::write(&actual_path, actual).expect("Failed to write actual snapshot");

        // Print diff instructions
        eprintln!("\n╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║ Snapshot Mismatch: {}                                ", test_name);
        eprintln!("╠══════════════════════════════════════════════════════════════╣");
        eprintln!("║ Expected: {}                                          ", snapshot_path);
        eprintln!("║ Actual:   {}                                      ", actual_path);
        eprintln!("╠══════════════════════════════════════════════════════════════╣");
        eprintln!("║ To review the difference:                                    ║");
        eprintln!("║   diff {} {}  ║", snapshot_path, actual_path);
        eprintln!("║                                                              ║");
        eprintln!("║ To accept the new snapshot:                                  ║");
        eprintln!("║   cp {} {}      ║", actual_path, snapshot_path);
        eprintln!("╚══════════════════════════════════════════════════════════════╝\n");

        panic!(
            "Snapshot mismatch for '{}'\nExpected: {}\nActual: {}",
            test_name, snapshot_path, actual_path
        );
    } else {
        // Clean up any stale .actual.snap file
        let _ = fs::remove_file(&actual_path);
    }
}
/// Delete a snapshot file (useful for cleanup in tests)
#[allow(dead_code)]
pub fn delete_snapshot(test_name: &str, snapshot_dir: &str) {
    let snapshot_path = format!("{}/{}.snap", snapshot_dir, test_name);
    let actual_path = format!("{}/{}.actual.snap", snapshot_dir, test_name);
    let _ = fs::remove_file(&snapshot_path);
    let _ = fs::remove_file(&actual_path);
}
