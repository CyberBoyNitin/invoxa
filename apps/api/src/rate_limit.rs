use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// In-process fixed-window rate limiter. This protects a single API instance;
/// a multi-instance deployment should put a shared limiter at the edge. That
/// limitation is documented in the deployment notes.
pub struct RateLimiter {
    buckets: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Returns `Some(retry_after_seconds)` when the request is over the limit.
    pub async fn check(&self, key: &str, limit: u32, window: Duration) -> Option<u64> {
        if limit == 0 {
            return Some(window.as_secs().max(1));
        }
        let now = Instant::now();
        let cutoff = now.checked_sub(window);
        let mut buckets = self.buckets.lock().await;
        if buckets.len() > 20_000 {
            buckets.retain(|_, values| {
                values
                    .back()
                    .map(|last| cutoff.map(|cut| *last >= cut).unwrap_or(true))
                    .unwrap_or(false)
            });
        }
        let values = buckets.entry(key.to_string()).or_default();
        while let Some(front) = values.front() {
            match cutoff {
                Some(cut) if *front < cut => {
                    values.pop_front();
                }
                _ => break,
            }
        }
        if values.len() >= limit as usize {
            let retry_after = values
                .front()
                .map(|first| {
                    first
                        .checked_add(window)
                        .and_then(|end| end.checked_duration_since(now))
                        .map(|duration| duration.as_secs().saturating_add(1))
                        .unwrap_or(1)
                })
                .unwrap_or(window.as_secs().max(1));
            return Some(retry_after);
        }
        values.push_back(now);
        None
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn blocks_after_limit_and_recovers() {
        let limiter = RateLimiter::new();
        for _ in 0..3 {
            assert!(limiter
                .check("key", 3, Duration::from_millis(60))
                .await
                .is_none());
        }
        assert!(limiter
            .check("key", 3, Duration::from_millis(60))
            .await
            .is_some());
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert!(limiter
            .check("key", 3, Duration::from_millis(60))
            .await
            .is_none());
    }

    #[tokio::test]
    async fn limits_are_per_key() {
        let limiter = RateLimiter::new();
        assert!(limiter
            .check("a", 1, Duration::from_secs(10))
            .await
            .is_none());
        assert!(limiter
            .check("b", 1, Duration::from_secs(10))
            .await
            .is_none());
        assert!(limiter
            .check("a", 1, Duration::from_secs(10))
            .await
            .is_some());
    }
}
