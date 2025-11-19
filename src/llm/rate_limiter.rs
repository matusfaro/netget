//! LLM rate limiter with concurrency control and token-based throttling
//!
//! This module provides a rate limiter that controls:
//! 1. **Concurrency**: Maximum number of concurrent LLM requests
//! 2. **Token usage**: Maximum tokens per time window (for API usage control)
//!
//! The rate limiter handles network events and user input differently:
//! - Network events: Discarded if rate limited (returns error immediately)
//! - User input: Waits until capacity is available

use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Semaphore, RwLock};
use tracing::{debug, info, warn};

/// Source of the LLM request
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestSource {
    /// User input (wait for capacity)
    User,
    /// Network event (discard if rate limited)
    Network,
}

/// Token usage record with timestamp
#[derive(Debug, Clone)]
struct TokenUsage {
    timestamp: Instant,
    input_tokens: u64,
    output_tokens: u64,
}

/// Rate limiter configuration
#[derive(Debug, Clone)]
pub struct RateLimiterConfig {
    /// Maximum concurrent LLM requests (default: 1)
    pub max_concurrent: usize,

    /// Maximum tokens per time window (None = unlimited)
    /// Default: None for local Ollama, recommended: 10000 for cloud APIs
    pub token_limit: Option<u64>,

    /// Time window for token limiting in seconds (default: 60)
    pub token_window_secs: u64,
}

impl Default for RateLimiterConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 1,
            token_limit: None,
            token_window_secs: 60,
        }
    }
}

/// LLM rate limiter with concurrency and token-based throttling
#[derive(Clone)]
pub struct RateLimiter {
    /// Concurrency control (wrapped in RwLock to allow semaphore replacement)
    semaphore: Arc<RwLock<Arc<Semaphore>>>,

    /// Configuration (can be updated at runtime)
    config: Arc<RwLock<RateLimiterConfig>>,

    /// Token usage history (protected by mutex for interior mutability)
    token_usage: Arc<Mutex<Vec<TokenUsage>>>,

    /// Statistics
    stats: Arc<Mutex<RateLimiterStats>>,
}

/// Rate limiter statistics
#[derive(Debug, Default, Clone)]
pub struct RateLimiterStats {
    /// Total requests attempted
    pub total_requests: u64,

    /// Requests completed
    pub requests_completed: u64,

    /// Requests discarded (network events that hit rate limit)
    pub requests_discarded: u64,

    /// Requests waiting (user inputs that are queued)
    pub requests_waiting: u64,

    /// Total input tokens processed
    pub total_input_tokens: u64,

    /// Total output tokens processed
    pub total_output_tokens: u64,

    /// Current tokens in window
    pub current_window_tokens: u64,
}

impl RateLimiter {
    /// Create a new rate limiter with the given configuration
    pub fn new(config: RateLimiterConfig) -> Self {
        let max_concurrent = config.max_concurrent;

        Self {
            semaphore: Arc::new(RwLock::new(Arc::new(Semaphore::new(max_concurrent)))),
            config: Arc::new(RwLock::new(config)),
            token_usage: Arc::new(Mutex::new(Vec::new())),
            stats: Arc::new(Mutex::new(RateLimiterStats::default())),
        }
    }

    /// Update the rate limiter configuration at runtime
    pub async fn update_config(&self, config: RateLimiterConfig) -> Result<()> {
        let mut current_config = self.config.write().await;

        // If max_concurrent changed, recreate the semaphore
        if config.max_concurrent != current_config.max_concurrent {
            info!(
                "Concurrency limit changed from {} to {} - recreating semaphore",
                current_config.max_concurrent,
                config.max_concurrent
            );

            // Replace the semaphore with a new one
            let mut semaphore = self.semaphore.write().await;
            *semaphore = Arc::new(Semaphore::new(config.max_concurrent));

            debug!(
                "Semaphore recreated with {} permits",
                config.max_concurrent
            );
        }

        *current_config = config;
        info!(
            "Rate limiter config updated: max_concurrent={}, token_limit={:?}, window={}s",
            current_config.max_concurrent,
            current_config.token_limit,
            current_config.token_window_secs
        );

        Ok(())
    }

    /// Get current configuration
    pub async fn get_config(&self) -> RateLimiterConfig {
        self.config.read().await.clone()
    }

    /// Get current statistics
    pub async fn get_stats(&self) -> RateLimiterStats {
        // Update current window tokens before returning stats
        let config = self.config.read().await;
        let mut stats = self.stats.lock().await;

        // Calculate tokens in current window
        let cutoff = Instant::now() - Duration::from_secs(config.token_window_secs);
        let token_usage = self.token_usage.lock().await;
        let window_tokens: u64 = token_usage
            .iter()
            .filter(|usage| usage.timestamp >= cutoff)
            .map(|usage| usage.input_tokens + usage.output_tokens)
            .sum();

        stats.current_window_tokens = window_tokens;
        stats.clone()
    }

    /// Check if we have token capacity available
    async fn check_token_capacity(&self) -> Result<bool> {
        let config = self.config.read().await;

        // If no token limit, always have capacity
        let Some(token_limit) = config.token_limit else {
            return Ok(true);
        };

        // Clean up old token usage records and calculate current usage
        let window_duration = Duration::from_secs(config.token_window_secs);
        let cutoff = Instant::now() - window_duration;

        let mut token_usage = self.token_usage.lock().await;

        // Remove old records
        token_usage.retain(|usage| usage.timestamp >= cutoff);

        // Calculate total tokens in current window
        let window_tokens: u64 = token_usage
            .iter()
            .map(|usage| usage.input_tokens + usage.output_tokens)
            .sum();

        debug!(
            "Token usage check: {}/{} tokens in {}s window",
            window_tokens, token_limit, config.token_window_secs
        );

        Ok(window_tokens < token_limit)
    }

    /// Record token usage after a successful LLM call
    pub async fn record_token_usage(&self, input_tokens: u64, output_tokens: u64) {
        let usage = TokenUsage {
            timestamp: Instant::now(),
            input_tokens,
            output_tokens,
        };

        // Add to history
        self.token_usage.lock().await.push(usage);

        // Update stats
        let mut stats = self.stats.lock().await;
        stats.total_input_tokens += input_tokens;
        stats.total_output_tokens += output_tokens;
        stats.requests_completed += 1;

        debug!(
            "Recorded token usage: input={}, output={}, total_requests={}",
            input_tokens, output_tokens, stats.requests_completed
        );
    }

    /// Acquire a permit for an LLM request
    ///
    /// - For `RequestSource::User`: Waits until capacity is available
    /// - For `RequestSource::Network`: Returns error immediately if rate limited
    ///
    /// Returns a permit guard that must be held for the duration of the LLM call.
    /// The permit is automatically released when the guard is dropped.
    pub async fn acquire_permit(
        &self,
        source: RequestSource,
    ) -> Result<RateLimiterPermit> {
        // Update stats
        {
            let mut stats = self.stats.lock().await;
            stats.total_requests += 1;
            if source == RequestSource::User {
                stats.requests_waiting += 1;
            }
        }

        debug!("Acquiring rate limiter permit for {:?} request", source);

        // Check token capacity
        let has_token_capacity = self.check_token_capacity().await?;

        if !has_token_capacity {
            match source {
                RequestSource::User => {
                    // For user requests, wait for token capacity
                    info!("Token limit reached, waiting for capacity (user request)");

                    // Poll until we have capacity (with exponential backoff)
                    let mut delay = Duration::from_millis(100);
                    while !self.check_token_capacity().await? {
                        tokio::time::sleep(delay).await;
                        delay = std::cmp::min(delay * 2, Duration::from_secs(5));
                    }

                    info!("Token capacity available, proceeding with user request");
                }
                RequestSource::Network => {
                    // For network requests, discard immediately
                    let mut stats = self.stats.lock().await;
                    stats.requests_discarded += 1;

                    let config = self.config.read().await;
                    warn!(
                        "Discarding network event: Token limit ({}/{} tokens in {}s window)",
                        self.token_usage.lock().await.iter()
                            .map(|u| u.input_tokens + u.output_tokens)
                            .sum::<u64>(),
                        config.token_limit.unwrap_or(0),
                        config.token_window_secs
                    );

                    anyhow::bail!(
                        "Rate limit exceeded: token limit reached (network event discarded)"
                    );
                }
            }
        }

        // Acquire concurrency permit
        let permit = match source {
            RequestSource::User => {
                // For user requests, wait for permit
                debug!("Waiting for concurrency permit (user request)");
                let semaphore = self.semaphore.read().await;
                Arc::clone(&*semaphore).acquire_owned().await
                    .context("Failed to acquire semaphore permit")?
            }
            RequestSource::Network => {
                // For network requests, try to acquire without waiting
                let semaphore = self.semaphore.read().await;
                match Arc::clone(&*semaphore).try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => {
                        let mut stats = self.stats.lock().await;
                        stats.requests_discarded += 1;

                        let config = self.config.read().await;
                        warn!(
                            "Discarding network event: Concurrency limit ({} concurrent requests)",
                            config.max_concurrent
                        );

                        anyhow::bail!(
                            "Rate limit exceeded: max concurrent requests (network event discarded)"
                        );
                    }
                }
            }
        };

        // Update stats
        {
            let mut stats = self.stats.lock().await;
            if source == RequestSource::User {
                stats.requests_waiting = stats.requests_waiting.saturating_sub(1);
            }
        }

        debug!("Rate limiter permit acquired for {:?} request", source);

        Ok(RateLimiterPermit {
            _permit: permit,
            rate_limiter: self.clone(),
        })
    }
}

/// RAII guard for a rate limiter permit
///
/// Automatically releases the concurrency permit when dropped.
pub struct RateLimiterPermit {
    _permit: tokio::sync::OwnedSemaphorePermit,
    rate_limiter: RateLimiter,
}

impl RateLimiterPermit {
    /// Record token usage for this request
    pub async fn record_usage(&self, input_tokens: u64, output_tokens: u64) {
        self.rate_limiter.record_token_usage(input_tokens, output_tokens).await;
    }
}
