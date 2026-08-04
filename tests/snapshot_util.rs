//! Unified snapshot testing utility
//!
//! Provides consistent snapshot testing across all test suites.
//! Uses .snap.md files for expected output and .actual.snap.md for mismatches.

use std::fs;

/// Assert that a value matches a snapshot file
///
/// # Snapshot File Naming
/// - `{test_name}.snap.md` - Expected snapshot (markdown format for better readability)
/// - `{test_name}.actual.snap.md` - Actual output when test fails (gitignored)
///
/// # Example
/// ```no_run
/// assert_snapshot("my_test", "snapshots", "test output");
/// ```
///
/// If the snapshot doesn't match:
/// 1. Creates `{test_name}.actual.snap.md` with actual output
/// 2. Prints diff instructions
/// 3. Panics with helpful message
pub fn assert_snapshot(test_name: &str, snapshot_dir: &str, actual: &str) {
    let snapshot_path = format!("{}/{}.snap.md", snapshot_dir, test_name);
    let actual_path = format!("{}/{}.actual.snap.md", snapshot_dir, test_name);

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

        let diff = first_diff_region(&expected, actual);

        // Print diff instructions
        eprintln!("\n╔══════════════════════════════════════════════════════════════╗");
        eprintln!(
            "║ Snapshot Mismatch: {}                                ",
            test_name
        );
        eprintln!("╠══════════════════════════════════════════════════════════════╣");
        eprintln!(
            "║ Expected: {}                                          ",
            snapshot_path
        );
        eprintln!(
            "║ Actual:   {}                                      ",
            actual_path
        );
        eprintln!("╠══════════════════════════════════════════════════════════════╣");
        eprintln!(
            "║ First differing region (line {}):                            ",
            diff.first_diff_line
        );
        eprintln!("╠══════════════════════════════════════════════════════════════╣");
        for line in &diff.rendered {
            eprintln!("{}", line);
        }
        eprintln!("╠══════════════════════════════════════════════════════════════╣");
        eprintln!("║ To review the full difference:                               ║");
        eprintln!("║   diff {} {}  ║", snapshot_path, actual_path);
        eprintln!("║                                                              ║");
        eprintln!("║ To accept the new snapshot:                                  ║");
        eprintln!("║   cp {} {}      ║", actual_path, snapshot_path);
        eprintln!("╚══════════════════════════════════════════════════════════════╝\n");

        panic!(
            "Snapshot mismatch for '{}'\nExpected: {}\nActual: {}\n\nFirst differing region (line {}):\n{}",
            test_name,
            snapshot_path,
            actual_path,
            diff.first_diff_line,
            diff.rendered.join("\n"),
        );
    } else {
        // Clean up any stale .actual.snap file
        let _ = fs::remove_file(&actual_path);
    }
}

/// A small, self-contained rendering of the first place two snapshot texts diverge.
struct DiffRegion {
    /// 1-based line number of the first differing line (relative to `expected`).
    first_diff_line: usize,
    /// Pre-formatted `-`/`+` lines ready to print, with a couple of lines of
    /// unchanged context before the divergence.
    rendered: Vec<String>,
}

/// Find and render the first differing region between two snapshot texts.
///
/// This is intentionally a minimal line-oriented diff (not a full LCS diff):
/// it walks both texts line-by-line until they diverge, then prints a little
/// context plus a bounded window of the differing lines from each side. That
/// is enough to see *where* and *how* a snapshot changed without needing to
/// manually run `diff` on the two files.
fn first_diff_region(expected: &str, actual: &str) -> DiffRegion {
    const CONTEXT_LINES: usize = 2;
    const MAX_DIFF_LINES: usize = 15;

    let expected_lines: Vec<&str> = expected.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();

    // Find the first index where the two line sequences differ.
    let common_len = expected_lines.len().min(actual_lines.len());
    let mut first_diff_idx = common_len;
    for i in 0..common_len {
        if expected_lines[i] != actual_lines[i] {
            first_diff_idx = i;
            break;
        }
    }

    let mut rendered = Vec::new();
    let context_start = first_diff_idx.saturating_sub(CONTEXT_LINES);
    for line in expected_lines
        .iter()
        .take(first_diff_idx)
        .skip(context_start)
    {
        rendered.push(format!("  {}", line));
    }

    let expected_end = (first_diff_idx + MAX_DIFF_LINES).min(expected_lines.len());
    for line in &expected_lines[first_diff_idx..expected_end] {
        rendered.push(format!("- {}", line));
    }
    if expected_end < expected_lines.len() {
        rendered.push(format!(
            "  … ({} more expected line(s) omitted)",
            expected_lines.len() - expected_end
        ));
    }

    let actual_end = (first_diff_idx + MAX_DIFF_LINES).min(actual_lines.len());
    for line in &actual_lines[first_diff_idx.min(actual_lines.len())..actual_end] {
        rendered.push(format!("+ {}", line));
    }
    if actual_end < actual_lines.len() {
        rendered.push(format!(
            "  … ({} more actual line(s) omitted)",
            actual_lines.len() - actual_end
        ));
    }

    if rendered.is_empty() {
        rendered.push("  (texts differ only in trailing whitespace/newline)".to_string());
    }

    DiffRegion {
        first_diff_line: first_diff_idx + 1,
        rendered,
    }
}
