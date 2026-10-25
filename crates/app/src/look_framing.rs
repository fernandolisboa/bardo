//! The look's framing (#243): how tall the face in a persona's look is,
//! measured on the computer with the take analysis' face tracker, once per
//! look image. The preflight check compares it with the takes' (half body
//! against full body): a motion-control model follows the look's framing,
//! so a take framed otherwise comes out stretched or cropped.

use std::sync::Arc;
use std::time::Duration;

use bardo_domain::{
    Job, JobFailure, JobFailureKind, JobKind, LookFaceMeasure, PersonaId, PersonaRepository,
    VisionError, VisionModels,
};
use bardo_media::MediaEngine;
use serde::{Deserialize, Serialize};

use crate::Bardo;
use crate::jobs::{JobContext, JobHandler};
use crate::performed_takes::look_print;
use crate::scenes::{id, parse, to_json, unexpected};

/// The longest side the look is measured at, in pixels: the face tracker
/// works on far less.
const MEASURE_SIDE: u32 = 1_280;

/// A look framing job's payload: the persona, and the print of the look
/// image it measures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct FramingPayload {
    persona: String,
    image: String,
}

fn vision_failure(error: VisionError) -> JobFailure {
    match error {
        VisionError::Missing(_) => JobFailure::new(JobFailureKind::Media, error.to_string()),
        _ => JobFailure::unexpected(error.to_string()),
    }
}

/// `(width, height)` scaled so its longer side is at most
/// [`MEASURE_SIDE`].
fn measure_size(width: u32, height: u32) -> (u32, u32) {
    let long = width.max(height).max(1);
    if long <= MEASURE_SIDE {
        return (width.max(1), height.max(1));
    }
    let scale = |side: u32| ((u64::from(side) * u64::from(MEASURE_SIDE)) / u64::from(long)).max(1);
    (scale(width) as u32, scale(height) as u32)
}

/// Measures the face in looks.
pub(crate) struct LookFramingHandler {
    pub(crate) personas: Arc<dyn PersonaRepository>,
    pub(crate) media: Arc<dyn MediaEngine>,
    pub(crate) vision: Arc<dyn VisionModels>,
}

impl JobHandler for LookFramingHandler {
    fn run(&self, payload: &str, _cx: &mut JobContext) -> Result<(), JobFailure> {
        let payload: FramingPayload = parse(payload)?;
        let persona: PersonaId = id(&payload.persona)?;
        // The look changed or went: a job for the new one measures it.
        let Some(look) = self.personas.look(persona).map_err(unexpected)? else {
            return Ok(());
        };
        let print = look_print(look.image.bytes());
        if print != payload.image {
            return Ok(());
        }
        if self
            .personas
            .look_face(persona)
            .map_err(unexpected)?
            .is_some_and(|measure| measure.image == print)
        {
            return Ok(());
        }
        let folder = tempfile::Builder::new()
            .prefix("bardo-look-")
            .tempdir()
            .map_err(|e| JobFailure::new(JobFailureKind::Files, e.to_string()))?;
        let source = folder
            .path()
            .join(format!("look.{}", look.image.format().extension()));
        std::fs::write(&source, look.image.bytes())
            .map_err(|e| JobFailure::new(JobFailureKind::Files, e.to_string()))?;
        let size = measure_size(look.image.width(), look.image.height());
        let frame = self
            .media
            .cover_picture(&source, size)
            .map_err(|e| JobFailure::new(JobFailureKind::Media, e.to_string()))?;
        let mut tracker = self.vision.face_tracker().map_err(vision_failure)?;
        let mut segmenter = self.vision.person_segmenter().map_err(vision_failure)?;
        let seen = {
            let mut head = || {
                Ok(segmenter
                    .segment(&frame)?
                    .head_regions(frame.width, frame.height)
                    .first()
                    .copied())
            };
            tracker
                .track(&frame, Duration::ZERO, &mut head)
                .map_err(vision_failure)?
        };
        let measure = LookFaceMeasure {
            image: print,
            face_height: seen.mesh.map(|mesh| mesh.height().clamp(0.0, 1.0)),
        };
        self.personas
            .save_look_face(persona, &measure)
            .map_err(unexpected)
    }
}

/// Queues the measure of `persona`'s look, whose print is `image`, unless
/// one is under way.
pub(crate) fn queue_look_framing(bardo: &Bardo, persona: PersonaId, image: &str) {
    let payload = FramingPayload {
        persona: persona.to_string(),
        image: image.to_owned(),
    };
    let queued = bardo.jobs().iter().any(|job| {
        job.kind() == JobKind::LookFraming
            && (job.state().is_active()
                // One that failed is left for the user to retry.
                || job.payload() == to_json(&payload))
    });
    if queued {
        return;
    }
    let job = Job::new(bardo.profile.id, JobKind::LookFraming, to_json(&payload));
    if let Err(error) = bardo.jobs.enqueue(job) {
        tracing::warn!(%error, "could not queue the look's framing");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_large_look_is_measured_at_most_1280_pixels_long() {
        assert_eq!(measure_size(720, 1280), (720, 1280));
        assert_eq!(measure_size(2048, 4096), (640, 1280));
        assert_eq!(measure_size(4000, 3000), (1280, 960));
        assert_eq!(measure_size(0, 0), (1, 1));
    }
}
