//! The built-in test job: counts through steps, saving a checkpoint after
//! each one. It exercises everything real jobs rely on (live progress,
//! cancel, retry with backoff, resume after restart) without a provider.

use std::time::Duration;

use bardo_domain::{JobFailure, JobFailureKind, Progress};
use serde::{Deserialize, Serialize};

use super::queue::{JobContext, JobHandler};

/// A test job to start from the jobs panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestJob {
    pub steps: u32,
    #[serde(rename = "step_ms", with = "millis")]
    pub step: Duration,
    /// Fail on purpose when reaching this step, on every attempt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fail_at_step: Option<u32>,
}

impl TestJob {
    /// Ten one-second steps.
    pub fn succeeding() -> Self {
        Self {
            steps: 10,
            step: Duration::from_secs(1),
            fail_at_step: None,
        }
    }

    /// Like `succeeding`, but fails halfway through.
    pub fn failing() -> Self {
        Self {
            fail_at_step: Some(5),
            ..Self::succeeding()
        }
    }

    pub(crate) fn payload(&self) -> String {
        serde_json::to_string(self).expect("a test job serializes")
    }
}

/// Progress updates per step, so the bar moves smoothly between checkpoints.
const TICKS_PER_STEP: u32 = 10;

pub(crate) struct Countdown;

impl JobHandler for Countdown {
    fn run(&self, payload: &str, cx: &mut JobContext) -> Result<(), JobFailure> {
        let job: TestJob = serde_json::from_str(payload)
            .map_err(|e| JobFailure::unexpected(format!("invalid test job payload: {e}")))?;
        let mut done: u32 = match cx.checkpoint() {
            Some(checkpoint) => checkpoint.parse().map_err(|e| {
                JobFailure::unexpected(format!("invalid checkpoint {checkpoint:?}: {e}"))
            })?,
            None => 0,
        };
        let total_ticks = u64::from(job.steps) * u64::from(TICKS_PER_STEP);
        let tick = job.step / TICKS_PER_STEP;

        while done < job.steps {
            if job.fail_at_step == Some(done) {
                return Err(JobFailure::new(
                    JobFailureKind::Simulated,
                    format!("test job failed on purpose at step {done}"),
                ));
            }
            for t in 1..=TICKS_PER_STEP {
                if !cx.sleep(tick) {
                    return Ok(());
                }
                let ticks = u64::from(done) * u64::from(TICKS_PER_STEP) + u64::from(t);
                cx.report_progress(Progress::of(ticks, total_ticks));
            }
            done += 1;
            cx.save_checkpoint(
                done.to_string(),
                Progress::of(u64::from(done), u64::from(job.steps)),
            )
            .map_err(|e| JobFailure::unexpected(format!("could not save the checkpoint: {e}")))?;
        }
        Ok(())
    }
}

mod millis {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
        u64::deserialize(deserializer).map(Duration::from_millis)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_round_trips() {
        for job in [TestJob::succeeding(), TestJob::failing()] {
            assert_eq!(
                serde_json::from_str::<TestJob>(&job.payload()).unwrap(),
                job
            );
        }
        assert_eq!(
            TestJob::succeeding().payload(),
            r#"{"steps":10,"step_ms":1000}"#
        );
    }
}
