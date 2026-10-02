//! Backoff for answers that mean "come back shortly": a rate limit (429),
//! an overloaded service (529) or another server error (5xx). Adapters
//! retry those a few times inside one call, waiting longer each time or as
//! long as the server's `retry-after` asks, before reporting a failure.
//! Longer outages are the job queue's to retry.

use std::sync::Arc;
use std::time::Duration;

use crate::http::{HttpRequest, HttpResponse, Transport, TransportError};

/// Waits between attempts. Tests pass one that records instead of
/// sleeping.
pub type Sleeper = Arc<dyn Fn(Duration) + Send + Sync>;

/// Sleeps the calling thread (a job's worker thread).
pub fn thread_sleeper() -> Sleeper {
    Arc::new(std::thread::sleep)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff {
    /// Attempts after the first.
    pub retries: u32,
    /// Wait before the first retry; each later wait doubles.
    pub first_delay: Duration,
    /// Upper bound for any wait, `retry-after` included, so a job stays
    /// responsive to cancel.
    pub max_delay: Duration,
}

impl Default for Backoff {
    /// Three retries, waiting 1 s, 2 s and 4 s unless told otherwise.
    fn default() -> Self {
        Self {
            retries: 3,
            first_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(10),
        }
    }
}

impl Backoff {
    /// Whether the answer asks to come back shortly.
    pub fn is_retryable(response: &HttpResponse) -> bool {
        response.status == 429 || (500..=599).contains(&response.status)
    }

    /// The wait before retry number `retry` (from 0): what `retry-after`
    /// asks (in seconds), else the doubling delay; capped either way.
    pub fn delay(&self, retry: u32, response: &HttpResponse) -> Duration {
        let asked = response
            .header("retry-after")
            .and_then(|value| value.trim().parse::<f64>().ok())
            .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
            .map(Duration::from_secs_f64);
        let doubling = self.first_delay.saturating_mul(1 << retry.min(31));
        asked.unwrap_or(doubling).min(self.max_delay)
    }

    /// Sends `request`, retrying retryable answers. Returns the last answer
    /// (retryable or not) or the transport error.
    pub fn send(
        &self,
        transport: &dyn Transport,
        request: &HttpRequest,
        sleep: &Sleeper,
    ) -> Result<HttpResponse, TransportError> {
        let mut retry = 0;
        loop {
            let response = transport.send(request)?;
            if retry >= self.retries || !Self::is_retryable(&response) {
                return Ok(response);
            }
            sleep(self.delay(retry, &response));
            retry += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delays_double_up_to_the_cap() {
        let backoff = Backoff::default();
        let busy = HttpResponse::new(529, "");
        let delays: Vec<_> = (0..5).map(|retry| backoff.delay(retry, &busy)).collect();
        assert_eq!(
            delays,
            [1, 2, 4, 8, 10].map(Duration::from_secs),
            "capped at max_delay"
        );
    }

    #[test]
    fn retry_after_wins_within_the_cap() {
        let backoff = Backoff::default();
        let asked = |value: &str| HttpResponse::new(429, "").with_header("Retry-After", value);
        assert_eq!(backoff.delay(0, &asked("3")), Duration::from_secs(3));
        assert_eq!(backoff.delay(0, &asked("0.5")), Duration::from_millis(500));
        assert_eq!(backoff.delay(0, &asked("120")), Duration::from_secs(10));
        assert_eq!(
            backoff.delay(1, &asked("Wed, 21 Oct 2026 07:28:00 GMT")),
            Duration::from_secs(2),
            "an HTTP date falls back to the doubling delay"
        );
    }

    #[test]
    fn only_throttling_and_server_errors_are_retried() {
        for status in [429, 500, 503, 529] {
            assert!(Backoff::is_retryable(&HttpResponse::new(status, "")));
        }
        for status in [200, 400, 401, 403, 404, 422] {
            assert!(!Backoff::is_retryable(&HttpResponse::new(status, "")));
        }
    }
}
