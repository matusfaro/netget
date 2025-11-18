//! System statistics monitoring

use sysinfo::{System, RefreshKind, CpuRefreshKind, MemoryRefreshKind};
use std::sync::Arc;
use tokio::sync::RwLock;

#[cfg(feature = "gpu")]
use gfxinfo::active_gpu;

/// System statistics (CPU, memory, GPU)
#[derive(Debug, Clone)]
pub struct SystemStats {
    /// CPU usage percentage (0-100)
    pub cpu_usage: f32,
    /// Memory usage in bytes
    pub memory_used: u64,
    /// Total memory in bytes
    pub memory_total: u64,
    /// GPU usage percentage (0-100, None if not available)
    pub gpu_usage: Option<f32>,
    /// GPU memory used in bytes (None if not available)
    pub gpu_memory_used: Option<u64>,
    /// GPU memory total in bytes (None if not available)
    pub gpu_memory_total: Option<u64>,
}

impl Default for SystemStats {
    fn default() -> Self {
        Self {
            cpu_usage: 0.0,
            memory_used: 0,
            memory_total: 0,
            gpu_usage: None,
            gpu_memory_used: None,
            gpu_memory_total: None,
        }
    }
}

impl SystemStats {
    /// Get memory usage percentage
    pub fn memory_percent(&self) -> f32 {
        if self.memory_total == 0 {
            0.0
        } else {
            (self.memory_used as f64 / self.memory_total as f64 * 100.0) as f32
        }
    }

    /// Format memory usage as human-readable string
    pub fn memory_used_str(&self) -> String {
        format_bytes(self.memory_used)
    }

    /// Format total memory as human-readable string
    pub fn memory_total_str(&self) -> String {
        format_bytes(self.memory_total)
    }

    /// Format GPU memory usage as human-readable string
    pub fn gpu_memory_used_str(&self) -> String {
        self.gpu_memory_used
            .map(format_bytes)
            .unwrap_or_else(|| "N/A".to_string())
    }

    /// Format GPU memory total as human-readable string
    pub fn gpu_memory_total_str(&self) -> String {
        self.gpu_memory_total
            .map(format_bytes)
            .unwrap_or_else(|| "N/A".to_string())
    }
}

/// Format bytes as human-readable string (KB, MB, GB, etc.)
fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit_index = 0;

    while value >= 1024.0 && unit_index < UNITS.len() - 1 {
        value /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} {}", value as u64, UNITS[unit_index])
    } else {
        format!("{:.1} {}", value, UNITS[unit_index])
    }
}

/// System statistics monitor that updates at most once per second
pub struct SystemStatsMonitor {
    system: Arc<RwLock<System>>,
    last_update: Arc<RwLock<std::time::Instant>>,
}

impl SystemStatsMonitor {
    /// Create a new system stats monitor
    pub fn new() -> Self {
        let system = System::new_with_specifics(
            RefreshKind::new()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything()),
        );

        Self {
            system: Arc::new(RwLock::new(system)),
            last_update: Arc::new(RwLock::new(std::time::Instant::now())),
        }
    }

    /// Get current system stats (updates at most once per second)
    pub async fn get_stats(&self) -> SystemStats {
        let now = std::time::Instant::now();
        let mut last_update = self.last_update.write().await;

        // Only update if more than 1 second has passed
        if now.duration_since(*last_update).as_secs() >= 1 {
            let mut system = self.system.write().await;
            system.refresh_cpu_all();
            system.refresh_memory();
            *last_update = now;
        }

        // Read stats
        let system = self.system.read().await;

        // Get GPU stats if available (cross-platform: NVIDIA, AMD, Intel)
        #[cfg(feature = "gpu")]
        let (gpu_usage, gpu_memory_used, gpu_memory_total) = {
            // active_gpu() returns Result<Box<dyn Gpu>, _>
            match active_gpu() {
                Ok(gpu) => {
                    let info = gpu.info();

                    // gfxinfo returns 0 for unsupported/unavailable stats
                    let gpu_usage = {
                        let load = info.load_pct();
                        if load > 0 { Some(load as f32) } else { None }
                    };
                    let gpu_memory_used = {
                        let used = info.used_vram();
                        if used > 0 { Some(used) } else { None }
                    };
                    let gpu_memory_total = {
                        let total = info.total_vram();
                        if total > 0 { Some(total) } else { None }
                    };

                    (gpu_usage, gpu_memory_used, gpu_memory_total)
                }
                Err(_) => {
                    // GPU detection failed (no GPU or drivers not available)
                    (None, None, None)
                }
            }
        };

        #[cfg(not(feature = "gpu"))]
        let (gpu_usage, gpu_memory_used, gpu_memory_total) = (None, None, None);

        SystemStats {
            cpu_usage: system.global_cpu_usage(),
            memory_used: system.used_memory(),
            memory_total: system.total_memory(),
            gpu_usage,
            gpu_memory_used,
            gpu_memory_total,
        }
    }
}

impl Default for SystemStatsMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
