//! Performed takes straight from Kling's own API (#243, ADR-0027): Kling
//! 3.0 Motion Control at 720p or 1080p, with drafts on Kling 2.6 Motion
//! Control at 720p, the cheapest tier Kling has.
//!
//! `POST /motion-control/{model}` takes the look (inline, as Base64), the
//! control video (as a link: Kling takes no video inline and has no upload
//! of its own, so the video is staged with another provider and read from
//! there) and the prompt, and returns a task id; `GET /tasks?task_ids=…`
//! polls it as for clips. Kling bills a take by the seconds of video it
//! returns, which can be shorter than the control video, so the cost is
//! recorded from the result's own length.
//!
//! Like clips, every submission carries a custom task id made from the
//! job's submission id and the request; a resubmission Kling refuses is
//! found by that id instead of being paid for again. The id is made from
//! the request as staged (the video's staged name, and the look's name,
//! which carries its print), not from the signed link, which changes each
//! time it is signed, nor from the look's bytes, which are encoded again
//! each time they are sent.

use std::time::Duration;

use bardo_domain::{
    ApiKey, ClipGenerator, ClipHandle, ClipModelRef, ClipStatus, GeneratedClip, Metered, Money,
    Provider, ProviderFailure, ProviderFailureKind, StagedFile, TakeDone, TakeGenerator, TakeInput,
    TakeMode, TakeModel, TakeRequest, TakeStatus, TakeTier,
};
use base64::Engine as _;
use serde_json::{Value, json};

use crate::http::{HttpRequest, HttpResponse, Transport, UreqTransport};
use crate::kling::{
    API_URL, KlingClips, auth, json_body, kling_failure, parse_status, parse_submission,
    status_request, succeeded, task_id_for, unexpected,
};

/// The take model a 720p take uses.
pub const TAKE_MODEL_720P: &str = "motion-control/kling-3.0/720p";

/// The take model a 1080p take uses.
pub const TAKE_MODEL_1080P: &str = "motion-control/kling-3.0/1080p";

/// What drafts are made with: Kling 2.6 Motion Control at 720p costs
/// about half of 3.0 at 720p.
pub const DRAFT_MODEL: &str = "motion-control/kling-2.6/720p";

/// Longest prompt passed on, as Kling documents.
const MAX_PROMPT_CHARS: usize = 2_500;

/// The take models Bardo offers through Kling, with the limits Kling's
/// Motion Control docs give (checked on 2026-10-25).
pub fn take_models() -> Vec<TakeModel> {
    [
        (TAKE_MODEL_720P, "Kling 3.0 Motion Control 720p", 720),
        (TAKE_MODEL_1080P, "Kling 3.0 Motion Control 1080p", 1080),
    ]
    .into_iter()
    .map(|(id, name, height)| TakeModel {
        id: ClipModelRef::new(Provider::Kling, id).expect("catalog models are valid"),
        name: name.to_owned(),
        // It takes the performer's own video.
        modes: vec![TakeMode::Recording],
        draft: Some(TakeTier {
            resolution: None,
            height: 720,
            model: Some(DRAFT_MODEL.to_owned()),
        }),
        final_tier: TakeTier {
            resolution: None,
            height,
            model: None,
        },
        // A final is a new take: a draft is a check the user may skip.
        finishes_draft: false,
        draft_lifetime: None,
        look_assets: false,
        token_frame_rate: None,
        // Its picture follows the look's framing.
        keeps_framing: false,
        // Up to 30 s with the character facing as in the video.
        min_length: Duration::from_secs(3),
        max_length: Duration::from_secs(30),
        max_video_bytes: 100_000_000,
        // Kling reads up to 50 MB; the look goes inside the request.
        max_image_bytes: 10_000_000,
    })
    .collect()
}

/// The endpoint's model and the resolution a request is made at: the
/// tier's own model for a draft, the take model's for a final.
fn endpoint(request: &TakeRequest) -> Option<(String, String)> {
    let model = take_models()
        .into_iter()
        .find(|model| model.id.model() == request.model)?;
    let priced = model.priced_model(request.step);
    let (path, resolution) = priced.rsplit_once('/')?;
    Some((path.to_owned(), resolution.to_owned()))
}

/// The body with the inputs as staged (the video's staged name, the look's
/// name, which carries its print): the same request always makes it.
fn stable_body(request: &TakeRequest) -> Option<Value> {
    let (_, resolution) = endpoint(request)?;
    let prompt: String = request.prompt.chars().take(MAX_PROMPT_CHARS).collect();
    Some(json!({
        "contents": [
            { "type": "prompt", "text": prompt },
            { "type": "image", "url": request.look.0 },
            { "type": "video", "url": request.video.0 },
        ],
        // The character moves and faces as the performer does, and the
        // take's voice is the only sound.
        "settings": {
            "character_orientation": "video",
            "audio": "off",
            "resolution": resolution,
        },
    }))
}

/// The custom task id of a submission: from the body as staged and the
/// model it goes to, since a draft and a final can differ in nothing else.
fn task_id(request: &TakeRequest, body: &Value, submission: &str) -> Option<String> {
    let (path, _) = endpoint(request)?;
    Some(task_id_for(
        &json!({ "model": path, "body": body }),
        submission,
    ))
}

/// The custom task id of a submission.
pub fn external_task_id(request: &TakeRequest, submission: &str) -> Option<String> {
    task_id(request, &stable_body(request)?, submission)
}

/// The JSON body a submission sends: the look as raw Base64, the video
/// from its link. `None` for a model Kling does not offer here or a
/// request without the look's bytes.
pub fn take_body(request: &TakeRequest, submission: &str) -> Option<Value> {
    let look = request.look_bytes.as_deref()?;
    let mut body = stable_body(request)?;
    body["options"] = json!({
        "external_task_id": task_id(request, &body, submission)?,
        "watermark_info": { "enabled": false },
    });
    body["contents"][1]["url"] = json!(base64::engine::general_purpose::STANDARD.encode(look));
    let video = request.video_link.as_deref().unwrap_or(&request.video.0);
    body["contents"][2]["url"] = json!(video);
    Some(body)
}

/// The request that starts a take.
pub fn take_submit_request(
    key: &ApiKey,
    request: &TakeRequest,
    submission: &str,
) -> Option<HttpRequest> {
    let (path, _) = endpoint(request)?;
    let body = take_body(request, submission)?;
    Some(auth(
        HttpRequest::post_json(format!("{API_URL}/{path}"), body.to_string()),
        key,
    ))
}

/// The seconds of video a finished task returned, rounded to the nearest
/// whole second as Kling bills Motion Control (at least one).
fn output_seconds(response: &HttpResponse) -> Option<u64> {
    let body = json_body(response).ok()?;
    let task = body["data"].as_array()?.first()?;
    let output = task["outputs"]
        .as_array()?
        .iter()
        .find(|output| output["type"].as_str() == Some("video"))?;
    let seconds = match &output["duration"] {
        Value::String(text) => text.trim().parse::<f64>().ok()?,
        other => other.as_f64()?,
    };
    (seconds.is_finite() && seconds > 0.0).then(|| (seconds.round() as u64).max(1))
}

/// Reads a take's status answer; a finished take says how long it is.
pub fn parse_take_status(response: &HttpResponse) -> Result<TakeStatus, ProviderFailure> {
    Ok(match parse_status(response)? {
        ClipStatus::Queued => TakeStatus::Queued,
        ClipStatus::Running => TakeStatus::Running,
        ClipStatus::Done { video } => TakeStatus::Done(TakeDone {
            video,
            usage: output_seconds(response).map(Metered::video_seconds),
            seed: None,
        }),
        ClipStatus::Failed(failure) => TakeStatus::Failed(failure),
    })
}

/// Makes takes with Kling's Motion Control models, over HTTPS.
pub type KlingTakes<T = UreqTransport> = KlingClips<T>;

impl<T: Transport> TakeGenerator for KlingClips<T> {
    fn provider(&self) -> Provider {
        Provider::Kling
    }

    fn models(&self) -> Vec<TakeModel> {
        take_models()
    }

    /// Kling keeps no files: the look goes inside the request and the
    /// video is staged elsewhere.
    fn stage(&self, _key: &ApiKey, _input: TakeInput<'_>) -> Result<StagedFile, ProviderFailure> {
        Err(ProviderFailure::new(
            ProviderFailureKind::NotAllowed,
            "Kling takes no files ahead of a take",
        ))
    }

    fn stages_video(&self) -> bool {
        false
    }

    fn inline_look(&self) -> bool {
        true
    }

    /// Kling quotes nothing before starting; the rate table prices takes.
    fn quote(&self, _key: &ApiKey, _request: &TakeRequest) -> Option<Money> {
        None
    }

    fn submit(
        &self,
        key: &ApiKey,
        request: &TakeRequest,
        submission: &str,
    ) -> Result<ClipHandle, ProviderFailure> {
        if request.look_bytes.is_none() {
            return Err(unexpected("the look did not come with the take"));
        }
        let http = take_submit_request(key, request, submission).ok_or_else(|| {
            ProviderFailure::new(
                ProviderFailureKind::NotAllowed,
                format!("{} is not a Kling take model", request.model),
            )
        })?;
        let response = self.send(&http)?;
        if succeeded(&response) {
            return parse_submission(&response);
        }
        let refused = kling_failure(&response);
        // A refused request may be this very submission sent a second time
        // (Kling took the first, its answer was lost): find the first.
        // Kling does not document how it refuses a repeated custom id, so
        // any refusal but the key's, a limit or an outage looks; the lookup
        // is one free request.
        let may_repeat = matches!(
            refused.kind,
            ProviderFailureKind::Unexpected
                | ProviderFailureKind::NotAllowed
                | ProviderFailureKind::Declined
        );
        if may_repeat
            && let Some(external) = external_task_id(request, submission)
            && let Some(handle) = self.find(key, &external)
        {
            return Ok(handle);
        }
        Err(refused)
    }

    fn status(&self, key: &ApiKey, handle: &ClipHandle) -> Result<TakeStatus, ProviderFailure> {
        let http = status_request(key, handle)
            .ok_or_else(|| unexpected(format!("{} is not a Kling task", handle.0)))?;
        let response = self.send(&http)?;
        if succeeded(&response) {
            parse_take_status(&response)
        } else {
            Err(kling_failure(&response))
        }
    }

    fn download(&self, key: &ApiKey, video: &str) -> Result<GeneratedClip, ProviderFailure> {
        ClipGenerator::download(self, key, video)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bardo_domain::TakeStep;

    #[test]
    fn a_draft_is_made_on_2_6_and_a_final_on_3_0_at_its_resolution() {
        let request = |model: &str, step| TakeRequest {
            model: model.into(),
            step,
            resolution: None,
            prompt: String::new(),
            video: StagedFile("bardo/takes/a.mp4".into()),
            look: StagedFile("inline:5d2e8c1a".into()),
            draft_task: None,
            video_link: None,
            look_bytes: Some(vec![1]),
        };
        let pair = |path: &str, resolution: &str| Some((path.to_owned(), resolution.to_owned()));
        assert_eq!(
            endpoint(&request(TAKE_MODEL_720P, TakeStep::Draft)),
            pair("motion-control/kling-2.6", "720p")
        );
        assert_eq!(
            endpoint(&request(TAKE_MODEL_1080P, TakeStep::Draft)),
            pair("motion-control/kling-2.6", "720p")
        );
        assert_eq!(
            endpoint(&request(TAKE_MODEL_1080P, TakeStep::Final)),
            pair("motion-control/kling-3.0", "1080p")
        );
        assert_eq!(endpoint(&request("kling-3.0/720p", TakeStep::Final)), None);
    }
}
