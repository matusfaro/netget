//! Unit tests for `netget::system_stats`.
//!
//! Migrated out of `src/system_stats.rs` — CLAUDE.md requires all tests to live
//! under `tests/` and reach internals through the public `netget::` API.

use netget::system_stats::{format_bytes, SystemStatsMonitor};

#[test]
fn test_format_bytes() {
    assert_eq!(format_bytes(0), "0 B");
    assert_eq!(format_bytes(500), "500 B");
    assert_eq!(format_bytes(1024), "1.0 KB");
    assert_eq!(format_bytes(1536), "1.5 KB");
    assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
    assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0 GB");
}

#[tokio::test]
async fn test_stats_monitor() {
    let monitor = SystemStatsMonitor::new();
    let stats = monitor.get_stats().await;

    // Basic sanity checks
    assert!(stats.cpu_usage >= 0.0 && stats.cpu_usage <= 100.0);
    assert!(stats.memory_used > 0);
    assert!(stats.memory_total > 0);
    assert!(stats.memory_used <= stats.memory_total);
}
