//! Backoff for answers that mean "come back shortly": a rate limit (429),
//! an overloaded service (529) or another server error (5xx). Adapters
//! retry those a few times inside one call, waiting longer each time or as
//! long as the server's `retry-after` asks, before reporting a failure.
//! Longer outages are the job queue's to retry.
//!
//! A call the provider bills for the work it does (a text, an image, a
//! narration) is sent with [`Backoff::send_paid`] instead (ADR-0030): it
//! goes again only after an answer that says nothing was started, and a
//! request that was sent and got no answer is a lost answer, not an
//! outage, because the provider may have done and billed the work.

use std::sync::Arc;
use std::time::Duration;

use bardo_domain::{ProviderFailure, ProviderFailureKind};

use crate::http::{BinaryResponse, HttpRequest, HttpResponse, Transport, TransportError};

/// Says whether the JSON body of an error answer is the provider's own
/// error report, as opposed to that of something in front of it. Each
/// paid adapter knows its provider's.
pub type OwnWords = fn(&serde_json::Value) -> bool;

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
    /// (retryable or not) or the transport error. For calls that cost
    /// nothing to repeat: reads, status checks, uploads that resume.
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

    /// Sends a call the provider bills for the work it does and answers
    /// in JSON. It goes again only after an answer that started nothing; a
    /// request that never left is `Unreachable`; one that was sent and got
    /// no answer, whose answer a gateway lost, or whose answer arrived
    /// unreadable, is `AnswerLost`. Any other answer comes back for the
    /// adapter to read. `own_words` tells the provider's error reports.
    pub fn send_paid(
        &self,
        transport: &dyn Transport,
        request: &HttpRequest,
        own_words: OwnWords,
        sleep: &Sleeper,
    ) -> Result<HttpResponse, ProviderFailure> {
        let mut retry = 0;
        loop {
            let response = transport.send(request).map_err(unanswered)?;
            match PaidAnswer::of(response.status, response.body.as_bytes(), own_words) {
                PaidAnswer::StartedNothing if retry < self.retries => {}
                PaidAnswer::Lost => return Err(lost_upstream(response.status)),
                PaidAnswer::StartedNothing | PaidAnswer::Told => {
                    // The work was done; what came of it cannot be read.
                    if (200..=299).contains(&response.status)
                        && serde_json::from_str::<serde_json::Value>(&response.body).is_err()
                    {
                        return Err(unreadable(response.status));
                    }
                    return Ok(response);
                }
            }
            sleep(self.delay(retry, &response));
            retry += 1;
        }
    }

    /// `send_paid` for a call answered with a file (audio), kept as bytes
    /// up to `max_bytes`.
    pub fn send_paid_for_bytes(
        &self,
        transport: &dyn Transport,
        request: &HttpRequest,
        max_bytes: u64,
        own_words: OwnWords,
        sleep: &Sleeper,
    ) -> Result<BinaryResponse, ProviderFailure> {
        let mut retry = 0;
        loop {
            let response = transport
                .send_for_bytes(request, max_bytes)
                .map_err(unanswered)?;
            match PaidAnswer::of(response.status, &response.bytes, own_words) {
                PaidAnswer::StartedNothing if retry < self.retries => {}
                PaidAnswer::Lost => return Err(lost_upstream(response.status)),
                PaidAnswer::StartedNothing | PaidAnswer::Told => {
                    // The work was done; nothing of it arrived.
                    if (200..=299).contains(&response.status) && response.bytes.is_empty() {
                        return Err(unreadable(response.status));
                    }
                    return Ok(response);
                }
            }
            // The bytes of an answer come without its headers.
            sleep(self.delay(retry, &HttpResponse::new(response.status, "")));
            retry += 1;
        }
    }
}

/// What an answer to a paid call says about the work asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaidAnswer {
    /// The provider started nothing and says to come back shortly: a rate
    /// limit (429), or its own word that it has no capacity (503) or is
    /// overloaded (529). Safe to send again.
    StartedNothing,
    /// A server error that is not the provider speaking: a gateway that
    /// passed the request on and lost the answer (502, 504, a proxy's
    /// 52x), or any 5xx without the provider's own error report, JSON
    /// from something in front of it included. The work may have been
    /// done and billed.
    Lost,
    /// Anything else: the result, a refusal, or the provider saying it
    /// failed on its side (a 500 in its own words).
    Told,
}

impl PaidAnswer {
    fn of(status: u16, body: &[u8], own_words: OwnWords) -> Self {
        // A gateway's page, an empty body or another service's JSON is not
        // the provider speaking.
        let own_words =
            || serde_json::from_slice::<serde_json::Value>(body).is_ok_and(|body| own_words(&body));
        match status {
            429 => PaidAnswer::StartedNothing,
            503 | 529 if own_words() => PaidAnswer::StartedNothing,
            500 if own_words() => PaidAnswer::Told,
            500..=599 => PaidAnswer::Lost,
            _ => PaidAnswer::Told,
        }
    }
}

/// The failure of a paid call that got no answer: `Unreachable` when the
/// request never left, so the queue may send it again; `AnswerLost` when
/// it was sent, or may have been.
pub fn unanswered(error: TransportError) -> ProviderFailure {
    if error.never_left() {
        ProviderFailure::new(ProviderFailureKind::Unreachable, error.0)
    } else {
        ProviderFailure::new(
            ProviderFailureKind::AnswerLost,
            format!(
                "no answer came back, so the provider may have done and billed the work ({})",
                error.0
            ),
        )
    }
}

fn unreadable(status: u16) -> ProviderFailure {
    ProviderFailure::new(
        ProviderFailureKind::AnswerLost,
        format!(
            "the answer could not be read, so the provider may have done and billed the work (HTTP {status})"
        ),
    )
}

fn lost_upstream(status: u16) -> ProviderFailure {
    ProviderFailure::new(
        ProviderFailureKind::AnswerLost,
        format!(
            "a gateway lost the provider's answer, so the provider may have done and billed the work (HTTP {status})"
        ),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use super::*;

    /// Answers in order and counts what was sent.
    struct Answers(
        Mutex<VecDeque<Result<HttpResponse, TransportError>>>,
        Mutex<u32>,
    );

    impl Answers {
        fn new(answers: Vec<Result<HttpResponse, TransportError>>) -> Self {
            Self(Mutex::new(answers.into()), Mutex::new(0))
        }

        /// Answers in the provider's own words: a JSON error body.
        fn statuses(statuses: &[u16]) -> Self {
            Self::with_body(statuses, r#"{"error":{"message":"busy"}}"#)
        }

        fn with_body(statuses: &[u16], body: &str) -> Self {
            Self::new(
                statuses
                    .iter()
                    .map(|&status| Ok(HttpResponse::new(status, body)))
                    .collect(),
            )
        }

        fn sent(&self) -> u32 {
            *self.1.lock().unwrap()
        }
    }

    impl Transport for Answers {
        fn send(&self, _: &HttpRequest) -> Result<HttpResponse, TransportError> {
            *self.1.lock().unwrap() += 1;
            self.0.lock().unwrap().pop_front().expect("an answer left")
        }
    }

    fn no_sleep() -> Sleeper {
        Arc::new(|_| {})
    }

    /// The test provider reports its errors as `{"error": {...}}`.
    fn own(body: &serde_json::Value) -> bool {
        body["error"].is_object()
    }

    fn paid() -> HttpRequest {
        HttpRequest::post_json("https://provider.test/v1/generate", "{}")
    }

    #[test]
    fn a_paid_call_goes_again_only_after_an_answer_that_started_nothing() {
        let transport = Answers::statuses(&[429, 503, 529, 200]);
        let response = Backoff::default()
            .send_paid(&transport, &paid(), own, &no_sleep())
            .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(transport.sent(), 4);

        // The provider failing on its side is told once and left to the
        // queue; a refusal is told once.
        for status in [500, 400, 401] {
            let transport = Answers::statuses(&[status]);
            let response = Backoff::default()
                .send_paid(&transport, &paid(), own, &no_sleep())
                .unwrap();
            assert_eq!(response.status, status);
            assert_eq!(transport.sent(), 1);
        }
    }

    #[test]
    fn a_paid_call_that_was_sent_and_got_no_answer_is_a_lost_answer() {
        let transport = Answers::new(vec![Err(TransportError::unanswered("timeout: global"))]);
        let failure = Backoff::default()
            .send_paid(&transport, &paid(), own, &no_sleep())
            .unwrap_err();
        assert_eq!(failure.kind, ProviderFailureKind::AnswerLost);
        assert!(failure.detail.contains("timeout: global"), "{failure}");
        assert_eq!(transport.sent(), 1, "never sent again by itself");
    }

    #[test]
    fn json_from_something_in_front_of_the_provider_is_not_the_provider_speaking() {
        // A gateway that speaks JSON says nothing about the work behind it.
        for status in [500, 503, 529] {
            let transport = Answers::with_body(&[status], r#"{"message":"Service Unavailable"}"#);
            let failure = Backoff::default()
                .send_paid(&transport, &paid(), own, &no_sleep())
                .unwrap_err();
            assert_eq!(failure.kind, ProviderFailureKind::AnswerLost, "{status}");
            assert_eq!(transport.sent(), 1, "{status}");
        }
    }

    #[test]
    fn a_success_that_cannot_be_read_is_a_lost_answer() {
        // Too long, not text or cut: the transport hands over what it has.
        for body in ["", r#"{"content":[{"type":"te"#] {
            let transport = Answers::with_body(&[200], body);
            let failure = Backoff::default()
                .send_paid(&transport, &paid(), own, &no_sleep())
                .unwrap_err();
            assert_eq!(failure.kind, ProviderFailureKind::AnswerLost, "{body:?}");
            assert!(failure.detail.contains("could not be read"), "{failure}");
        }
        let transport = Answers::with_body(&[200], "");
        let failure = Backoff::default()
            .send_paid_for_bytes(&transport, &paid(), 1024, own, &no_sleep())
            .unwrap_err();
        assert_eq!(failure.kind, ProviderFailureKind::AnswerLost);
    }

    #[test]
    fn a_paid_call_that_never_left_is_unreachable() {
        let transport = Answers::new(vec![Err(TransportError::not_sent("host not found"))]);
        let failure = Backoff::default()
            .send_paid(&transport, &paid(), own, &no_sleep())
            .unwrap_err();
        assert_eq!(failure.kind, ProviderFailureKind::Unreachable);
        assert_eq!(failure.detail, "host not found");
    }

    #[test]
    fn a_gateway_that_lost_the_providers_answer_is_a_lost_answer() {
        for status in [502, 504, 520, 524] {
            let transport = Answers::statuses(&[status]);
            let failure = Backoff::default()
                .send_paid(&transport, &paid(), own, &no_sleep())
                .unwrap_err();
            assert_eq!(failure.kind, ProviderFailureKind::AnswerLost, "{status}");
            assert!(failure.detail.contains(&status.to_string()), "{failure}");
            assert_eq!(transport.sent(), 1, "{status}");
        }
    }

    #[test]
    fn a_server_error_that_is_not_the_provider_speaking_is_a_lost_answer() {
        // A proxy in front answers 503 after the provider dropped the
        // connection mid-work; the provider's own 503 is a JSON error.
        for (status, body) in [
            (
                503,
                "upstream connect error or disconnect/reset before headers",
            ),
            (500, "<html><body>Internal Server Error</body></html>"),
            (529, ""),
        ] {
            let transport = Answers::with_body(&[status], body);
            let failure = Backoff::default()
                .send_paid(&transport, &paid(), own, &no_sleep())
                .unwrap_err();
            assert_eq!(failure.kind, ProviderFailureKind::AnswerLost, "{status}");
            assert_eq!(transport.sent(), 1, "{status}");
        }
        // A rate limit is told before any work, whoever tells it.
        let transport = Answers::new(vec![
            Ok(HttpResponse::new(429, "Too Many Requests")),
            Ok(HttpResponse::new(200, "{}")),
        ]);
        Backoff::default()
            .send_paid(&transport, &paid(), own, &no_sleep())
            .unwrap();
        assert_eq!(transport.sent(), 2);
    }

    #[test]
    fn a_paid_call_for_bytes_follows_the_same_rules() {
        let transport = Answers::statuses(&[429, 200]);
        let response = Backoff::default()
            .send_paid_for_bytes(&transport, &paid(), 1024, own, &no_sleep())
            .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(transport.sent(), 2);

        let transport = Answers::statuses(&[504]);
        let failure = Backoff::default()
            .send_paid_for_bytes(&transport, &paid(), 1024, own, &no_sleep())
            .unwrap_err();
        assert_eq!(failure.kind, ProviderFailureKind::AnswerLost);

        let transport = Answers::new(vec![Err(TransportError::unanswered("reset"))]);
        let failure = Backoff::default()
            .send_paid_for_bytes(&transport, &paid(), 1024, own, &no_sleep())
            .unwrap_err();
        assert_eq!(failure.kind, ProviderFailureKind::AnswerLost);
        assert_eq!(transport.sent(), 1);
    }

    #[test]
    fn a_lasting_overload_comes_back_as_the_last_answer() {
        let transport = Answers::statuses(&[529, 529, 529, 529]);
        let response = Backoff::default()
            .send_paid(&transport, &paid(), own, &no_sleep())
            .unwrap();
        assert_eq!(response.status, 529);
        assert_eq!(transport.sent(), 4);
    }

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
