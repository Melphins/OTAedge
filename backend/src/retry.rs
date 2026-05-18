use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, warn};

/// Retry configuration for transient error handling
#[derive(Clone, Debug)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_attempts: u32,
    /// Initial delay before first retry
    pub initial_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Multiplier for exponential backoff
    pub backoff_factor: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(2),
            backoff_factor: 2.0,
        }
    }
}

/// Retry configuration for different operations
pub const RETRY_CONFIGS: RetryConfig = RetryConfig {
    max_attempts: 3,
    initial_delay: Duration::from_millis(100),
    max_delay: Duration::from_secs(2),
    backoff_factor: 2.0,
};

/// Execute an async operation with exponential backoff retry
pub async fn retry<F, Fut, T, E>(
    config: &RetryConfig,
    operation: F,
    operation_name: &str,
) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Display + Clone,
{
    let mut attempt = 0;
    let mut delay = config.initial_delay;

    loop {
        attempt += 1;

        match operation().await {
            Ok(result) => {
                if attempt > 1 {
                    debug!("{} succeeded after {} attempts", operation_name, attempt);
                }
                return Ok(result);
            }
            Err(ref e) if should_retry(e) && attempt < config.max_attempts => {
                warn!(
                    "{} failed (attempt {}/{}): {}. Retrying in {:?}...",
                    operation_name, attempt, config.max_attempts, e, delay
                );
                sleep(delay).await;
                delay = Duration::from_secs_f64(
                    (delay.as_secs_f64() * config.backoff_factor)
                        .min(config.max_delay.as_secs_f64()),
                );
            }
            Err(e) => {
                if attempt > 1 {
                    warn!(
                        "{} failed after {} attempts: {}",
                        operation_name, attempt, e
                    );
                }
                return Err(e);
            }
        }
    }
}

/// Determine if an error is transient and should be retried
fn should_retry<E: std::fmt::Display>(error: &E) -> bool {
    let error_str = error.to_string().to_lowercase();

    // Database connection errors
    if error_str.contains("connection") || error_str.contains("timeout") {
        return true;
    }

    // S3/network errors
    if error_str.contains("timeout")
        || error_str.contains("network")
        || error_str.contains("unreachable")
    {
        return true;
    }

    // IO errors that might be transient
    if error_str.contains("interrupted") || error_str.contains("try again") {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_retry_success() {
        let config = RetryConfig {
            max_attempts: 3,
            initial_delay: Duration::from_millis(10),
            max_delay: Duration::from_millis(100),
            backoff_factor: 2.0,
        };

        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_clone = attempts.clone();
        let result: Result<String, &str> = retry(
            &config,
            move || {
                let current = attempts_clone.fetch_add(1, Ordering::SeqCst) + 1;
                async move {
                    if current < 2 {
                        Err("connection timeout")
                    } else {
                        Ok("success".to_string())
                    }
                }
            },
            "test_operation",
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_retry_max_attempts() {
        let config = RetryConfig {
            max_attempts: 2,
            initial_delay: Duration::from_millis(10),
            max_delay: Duration::from_millis(100),
            backoff_factor: 2.0,
        };

        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_clone = attempts.clone();
        let result: Result<String, &str> = retry(
            &config,
            move || {
                attempts_clone.fetch_add(1, Ordering::SeqCst);
                async move { Err("connection timeout") }
            },
            "test_operation",
        )
        .await;

        assert!(result.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }
}
