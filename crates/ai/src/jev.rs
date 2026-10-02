//! The decision engine through JEV, TypeSafe's System One model
//! (`POST /v1/systemone`, docs.typesafe.ai). One call evaluates a state
//! against every question at once; choice and score map to TypeSafe's
//! question types of the same name, yes/no to its `noul` type.
//!
//! Rate limits (429) and overload (529) are retried with backoff, honoring
//! `retry-after`, as TypeSafe asks of direct HTTP callers.

use std::collections::HashMap;
use std::time::Duration;

use bardo_domain::{
    Answer, ApiKey, ChoiceAnswer, Confidence, DecisionEngine, Decisions, Provider, ProviderFailure,
    ProviderFailureKind, Question, Questions, ScoreAnswer, YesNoAnswer,
};
use serde_json::{Map, Value, json};

use crate::http::{HttpRequest, HttpResponse, Transport, UreqTransport};
use crate::key_check::failure;
use crate::retry::{Backoff, Sleeper, thread_sleeper};

pub const SYSTEM_ONE_URL: &str = "https://api.typesafe.ai/v1/systemone";
/// The latest stable JEV. Rankings are weighed in code, not thresholded on
/// a version's calibration, so following the alias is safe; each answer
/// records the version that gave it.
pub const MODEL: &str = "jev-latest";

/// Longest part of an invalid-request answer passed on.
const MAX_DETAIL_CHARS: usize = 300;

fn question_json(question: &Question) -> Value {
    match question {
        Question::Choice {
            instructions,
            options,
        } => {
            let criteria: Map<String, Value> = options
                .iter()
                .map(|(key, description)| (key.clone(), json!(description)))
                .collect();
            json!({ "type": "choice", "instructions": instructions, "criteria": criteria })
        }
        Question::Score {
            instructions,
            levels,
        } => json!({ "type": "score", "instructions": instructions, "criteria": levels }),
        Question::YesNo {
            instructions,
            yes,
            no,
        } => {
            let mut question = json!({ "type": "noul", "instructions": instructions });
            let mut criteria = Map::new();
            if let Some(yes) = yes {
                criteria.insert("true".into(), json!(yes));
            }
            if let Some(no) = no {
                criteria.insert("false".into(), json!(no));
            }
            if !criteria.is_empty() {
                question["criteria"] = Value::Object(criteria);
            }
            question
        }
    }
}

/// The System One request asking `questions` about `state`.
pub fn decision_request(key: &ApiKey, state: &str, questions: &Questions) -> HttpRequest {
    let questions: Map<String, Value> = questions
        .iter()
        .map(|(id, question)| (id.to_owned(), question_json(question)))
        .collect();
    let body = json!({ "model": MODEL, "state": state, "questions": questions });
    HttpRequest::post_json(SYSTEM_ONE_URL, body.to_string())
        .header("authorization", format!("Bearer {}", key.expose()))
}

fn unexpected(detail: impl Into<String>) -> ProviderFailure {
    ProviderFailure::new(ProviderFailureKind::Unexpected, detail)
}

fn confidence(answer: &Value, id: &str) -> Result<Confidence, ProviderFailure> {
    answer["confidence"]
        .as_f64()
        .map(Confidence::new)
        .ok_or_else(|| unexpected(format!("answer {id} has no confidence")))
}

fn parse_answer(id: &str, question: &Question, answer: &Value) -> Result<Answer, ProviderFailure> {
    let wrong = || unexpected(format!("answer {id} does not match its question"));
    match question {
        Question::Choice { options, .. } => {
            if answer["type"] != "choice" {
                return Err(wrong());
            }
            let choice = answer["choice"].as_str().ok_or_else(wrong)?;
            if !options.iter().any(|(key, _)| key == choice) {
                return Err(unexpected(format!("answer {id} chose an unknown option")));
            }
            let probabilities = answer["probabilities"]
                .as_object()
                .ok_or_else(wrong)?
                .iter()
                .filter_map(|(option, p)| Some((option.clone(), p.as_f64()?)))
                .collect();
            Ok(Answer::Choice(ChoiceAnswer {
                choice: choice.to_owned(),
                probabilities,
                confidence: confidence(answer, id)?,
            }))
        }
        Question::Score { levels, .. } => {
            if answer["type"] != "score" {
                return Err(wrong());
            }
            let level = answer["score"].as_f64().ok_or_else(wrong)?;
            let by_level = answer["probabilities"].as_object().ok_or_else(wrong)?;
            let probabilities = (0..levels.len())
                .map(|index| by_level.get(&index.to_string()).and_then(Value::as_f64))
                .map(|p| p.unwrap_or(0.0))
                .collect();
            Ok(Answer::Score(ScoreAnswer {
                level,
                probabilities,
                confidence: confidence(answer, id)?,
            }))
        }
        Question::YesNo { .. } => {
            if answer["type"] != "noul" {
                return Err(wrong());
            }
            let yes = answer["noul"].as_f64().ok_or_else(wrong)?;
            Ok(Answer::YesNo(YesNoAnswer::new(yes)))
        }
    }
}

/// Reads a successful answer: one typed answer per question asked.
pub fn parse_decisions(
    questions: &Questions,
    response: &HttpResponse,
) -> Result<Decisions, ProviderFailure> {
    let body: Value = serde_json::from_str(&response.body)
        .map_err(|error| unexpected(format!("unreadable response: {error}")))?;
    let mut answers = HashMap::new();
    for (id, question) in questions.iter() {
        let answer = &body["answers"][id];
        if answer.is_null() {
            return Err(unexpected(format!("no answer for {id}")));
        }
        answers.insert(id.to_owned(), parse_answer(id, question, answer)?);
    }
    Ok(Decisions {
        answers,
        model: body["model"].as_str().unwrap_or(MODEL).to_owned(),
    })
}

fn cut(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= MAX_DETAIL_CHARS {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(MAX_DETAIL_CHARS).collect();
    cut.push('…');
    cut
}

/// Asks JEV over HTTPS.
pub struct JevDecisionEngine<T = UreqTransport> {
    transport: T,
    backoff: Backoff,
    sleep: Sleeper,
}

impl JevDecisionEngine {
    /// JEV answers in about a second; the margin covers a slow network.
    pub const TIMEOUT: Duration = Duration::from_secs(60);

    pub fn new() -> Self {
        Self::with_transport(UreqTransport::new(Self::TIMEOUT))
    }
}

impl Default for JevDecisionEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transport> JevDecisionEngine<T> {
    pub fn with_transport(transport: T) -> Self {
        Self {
            transport,
            backoff: Backoff::default(),
            sleep: thread_sleeper(),
        }
    }

    /// Replaces how the adapter waits between retries (tests record the
    /// waits instead).
    pub fn with_sleeper(mut self, sleep: Sleeper) -> Self {
        self.sleep = sleep;
        self
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }
}

impl<T: Transport> DecisionEngine for JevDecisionEngine<T> {
    fn decide(
        &self,
        key: &ApiKey,
        state: &str,
        questions: &Questions,
    ) -> Result<Decisions, ProviderFailure> {
        if questions.is_empty() {
            return Ok(Decisions {
                answers: HashMap::new(),
                model: MODEL.to_owned(),
            });
        }
        let request = decision_request(key, state, questions);
        let response = self
            .backoff
            .send(&self.transport, &request, &self.sleep)
            .map_err(|error| ProviderFailure::new(ProviderFailureKind::Unreachable, error.0))?;
        match response.status {
            200..=299 => parse_decisions(questions, &response),
            // The body names the offending field; it is Bardo's bug.
            422 => Err(unexpected(format!(
                "request rejected as invalid: {}",
                cut(&response.body)
            ))),
            _ => Err(failure(Provider::TypeSafe, &response)),
        }
    }
}
