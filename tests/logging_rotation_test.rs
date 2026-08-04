//! Tests for the size-based rotating log file writer
//! (`netget::logging::RotatingFileWriter`).
//!
//! These drive the rotator directly against files in a `tempfile::tempdir`
//! — never the repo's real `netget.log` — verifying that:
//!   - the active file rotates out once it reaches the configured size cap
//!   - retention is bounded: only `max_files` rotated files are kept, and
//!     the oldest generation is dropped once that count is exceeded
//!   - concurrent writers (many threads, standing in for many tokio tasks)
//!     never interleave/corrupt lines or lose writes, including while
//!     rotation is happening mid-stream
//!   - opening on top of an already-oversized existing file (e.g. today's
//!     unbounded `netget.log`) never truncates or deletes it: the existing
//!     content is rotated out intact on the first write, not discarded

use netget::logging::RotatingFileWriter;
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Barrier};
use std::thread;

/// Path of the Nth rotated file for `path`, matching the writer's internal
/// naming scheme (`<path>.1`, `<path>.2`, ...).
fn rotated_path(path: &std::path::Path, n: usize) -> PathBuf {
    PathBuf::from(format!("{}.{n}", path.display()))
}

/// Fixed-width 10-byte chunk: "chunk-000\n" .. "chunk-011\n".
fn chunk(i: usize) -> String {
    format!("chunk-{i:03}\n")
}

#[test]
fn test_rotation_and_retention_drops_oldest() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("test.log");

    // 3 chunks (10 bytes each) fill one generation; retain 2 rotated files
    // plus the active file, so 3 of the 4 generations written should survive.
    let max_bytes = 30u64;
    let max_files = 2usize;

    let mut writer =
        RotatingFileWriter::with_limits(&path, max_bytes, max_files).expect("open writer");

    for i in 0..12 {
        write!(writer, "{}", chunk(i)).expect("write chunk");
    }
    writer.flush().expect("flush");

    let active = fs::read_to_string(&path).expect("read active file");
    let rot1 = fs::read_to_string(rotated_path(&path, 1)).expect("read .1");
    let rot2 = fs::read_to_string(rotated_path(&path, 2)).expect("read .2");

    assert_eq!(active, "chunk-009\nchunk-010\nchunk-011\n", "active file");
    assert_eq!(rot1, "chunk-006\nchunk-007\nchunk-008\n", "rotated file .1");
    assert_eq!(rot2, "chunk-003\nchunk-004\nchunk-005\n", "rotated file .2");

    // Retention must be enforced: no .3 file, and the oldest generation
    // (chunk-000..chunk-002) must be gone, not lingering anywhere.
    assert!(
        !rotated_path(&path, 3).exists(),
        "retention count exceeded: found a .3 file beyond max_files={max_files}"
    );
    assert!(!active.contains("chunk-000"));
    assert!(!rot1.contains("chunk-000"));
    assert!(!rot2.contains("chunk-000"));

    // Hard ceiling check: total bytes on disk for this log must never
    // exceed (max_files + 1) * max_bytes.
    let total: u64 = [
        Some(path.clone()),
        Some(rotated_path(&path, 1)),
        Some(rotated_path(&path, 2)),
    ]
    .into_iter()
    .flatten()
    .map(|p| fs::metadata(p).map(|m| m.len()).unwrap_or(0))
    .sum();
    assert!(
        total <= (max_files as u64 + 1) * max_bytes,
        "total log bytes {total} exceeded hard ceiling {}",
        (max_files as u64 + 1) * max_bytes
    );
}

#[test]
fn test_concurrent_writes_are_not_corrupted_or_lost_across_rotations() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("concurrent.log");

    // Small enough that rotation happens repeatedly during the run, but
    // generous enough (with max_files) that nothing gets dropped, so every
    // line written must still be recoverable afterwards.
    let max_bytes = 500u64;
    let max_files = 20usize;

    let writer = RotatingFileWriter::with_limits(&path, max_bytes, max_files).expect("open writer");

    let n_threads = 8usize;
    let lines_per_thread = 50usize;
    let barrier = Arc::new(Barrier::new(n_threads));

    let handles: Vec<_> = (0..n_threads)
        .map(|t| {
            let mut w = writer.clone();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                for i in 0..lines_per_thread {
                    // Fixed-width record so any interleaving/corruption is
                    // immediately detectable by length or content.
                    write!(w, "T{t:02}-L{i:03}-PAYLOAD\n").expect("write line");
                }
            })
        })
        .collect();

    for h in handles {
        h.join().expect("writer thread panicked");
    }
    writer.clone().flush().expect("flush");

    // Reassemble every generation still on disk (rotation is contiguous
    // starting at .1, so stop at the first missing index).
    let mut all_content = String::new();
    let mut n = 1;
    loop {
        let p = rotated_path(&path, n);
        if !p.exists() {
            break;
        }
        all_content.push_str(&fs::read_to_string(&p).expect("read rotated file"));
        n += 1;
    }
    all_content.push_str(&fs::read_to_string(&path).expect("read active file"));

    assert!(
        all_content.ends_with('\n'),
        "trailing partial/torn line detected: {:?}",
        all_content.rsplit('\n').next()
    );

    let expected_line_len = format!("T{:02}-L{:03}-PAYLOAD", 0, 0).len();
    let mut seen = HashSet::new();
    for line in all_content.lines() {
        assert_eq!(
            line.len(),
            expected_line_len,
            "corrupted or interleaved line (wrong length): {line:?}"
        );
        assert!(line.starts_with('T') && line.contains("-L") && line.ends_with("-PAYLOAD"));
        assert!(
            seen.insert(line.to_string()),
            "duplicate line found (would indicate lost/re-rotated data): {line:?}"
        );
    }

    assert_eq!(
        seen.len(),
        n_threads * lines_per_thread,
        "expected every concurrently-written line to survive rotation intact"
    );
}

#[test]
fn test_opening_oversized_existing_file_preserves_data_via_rotation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("preexisting.log");

    // Simulate an already-oversized log file (like today's unbounded
    // netget.log) that predates the rotator being wired in.
    let old_data = "OLD-DATA-FROM-BEFORE-ROTATION-WAS-ADDED\n".repeat(5);
    fs::write(&path, &old_data).expect("seed pre-existing oversized file");
    assert!(
        old_data.len() as u64 > 50,
        "sanity: seed data must exceed max_bytes below"
    );

    let max_bytes = 50u64;
    let max_files = 1usize;
    let mut writer =
        RotatingFileWriter::with_limits(&path, max_bytes, max_files).expect("open writer");

    // First write after opening must trigger rotation (existing file is
    // already >= max_bytes) rather than truncating/deleting it.
    write!(writer, "NEW\n").expect("write");
    writer.flush().expect("flush");

    let rotated = fs::read_to_string(rotated_path(&path, 1)).expect("read rotated-out old file");
    assert_eq!(
        rotated, old_data,
        "pre-existing oversized log content must be preserved intact in the rotated file"
    );

    let active = fs::read_to_string(&path).expect("read active file");
    assert_eq!(
        active, "NEW\n",
        "active file should only contain the new write"
    );
}
