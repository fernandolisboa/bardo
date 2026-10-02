//! The JEV decision engine against recorded responses (see
//! `fixtures/jev/README.md`). No test calls TypeSafe.

mod common;

use std::time::Duration;

use bardo_ai::JevDecisionEngine;
use bardo_ai::http::Method;
use bardo_ai::jev::{MODEL, SYSTEM_ONE_URL};
use bardo_domain::{
    ApiKey, DecisionEngine, Provider, ProviderFailureKind, Question, Questions, Score,
};
use common::{Scripted, fixture, recording_sleeper};
use serde_json::json;

const KEY: &str = "ts-test-key-0001-abcdef";
const STATE: &str = "Channel: Space Archives. Niche: space history.";

fn key() -> ApiKey {
    ApiKey::parse(Provider::TypeSafe, KEY).unwrap()
}

fn questions() -> Questions {
    Questions::new()
        .ask(
            "fit",
            Question::Score {
                instructions: "How well does the idea fit the channel?".into(),
                levels: [
                    "Off-brand",
                    "Loosely related",
                    "Related",
                    "Fits well",
                    "Perfect fit",
                ]
                .map(String::from)
                .to_vec(),
            },
        )
        .unwrap()
        .ask(
            "format",
            Question::Choice {
                instructions: "Which format suits the idea?".into(),
                options: vec![
                    ("documentary".into(), Some("One story told in depth".into())),
                    ("listicle".into(), None),
                ],
            },
        )
        .unwrap()
        .ask(
            "safe",
            Question::YesNo {
                instructions: "Is the idea safe for advertisers?".into(),
                yes: Some("No sensitive topics".into()),
                no: None,
            },
        )
        .unwrap()
}

fn engine(answers: &[&str]) -> JevDecisionEngine<Scripted> {
    let answers = answers.iter().map(|name| fixture("jev", name)).collect();
    let (sleeper, _) = recording_sleeper();
    JevDecisionEngine::with_transport(Scripted::new(answers)).with_sleeper(sleeper)
}

#[test]
fn sends_every_question_in_one_system_one_call() {
    let jev = engine(&["systemone-answers"]);
    jev.decide(&key(), STATE, &questions()).unwrap();

    let sent = jev.transport().sent();
    assert_eq!(sent.len(), 1);
    let sent = &sent[0];
    assert_eq!(sent.method, Method::Post);
    assert_eq!(sent.url, SYSTEM_ONE_URL);
    assert_eq!(
        sent.header("authorization"),
        Some(&*format!("Bearer {KEY}"))
    );
    assert_eq!(sent.body["model"], MODEL);
    assert_eq!(sent.body["state"], STATE);
    let asked = &sent.body["questions"];
    assert_eq!(
        asked["fit"],
        json!({
            "type": "score",
            "instructions": "How well does the idea fit the channel?",
            "criteria": ["Off-brand", "Loosely related", "Related", "Fits well", "Perfect fit"],
        })
    );
    assert_eq!(
        asked["format"],
        json!({
            "type": "choice",
            "instructions": "Which format suits the idea?",
            "criteria": {"documentary": "One story told in depth", "listicle": null},
        })
    );
    assert_eq!(
        asked["safe"],
        json!({
            "type": "noul",
            "instructions": "Is the idea safe for advertisers?",
            "criteria": {"true": "No sensitive topics"},
        })
    );
}

#[test]
fn reads_typed_answers_with_probabilities_and_confidence() {
    let decisions = engine(&["systemone-answers"])
        .decide(&key(), STATE, &questions())
        .unwrap();
    assert_eq!(decisions.model, "jev-1.13.0");

    let fit = decisions.score("fit").unwrap();
    assert_eq!(fit.level, 3.2);
    assert_eq!(fit.probabilities, [0.0, 0.02, 0.1, 0.52, 0.36]);
    assert_eq!(fit.confidence.percent(), 81);
    assert_eq!(fit.normalized(), Score::new(80));

    let format = decisions.choice("format").unwrap();
    assert_eq!(format.choice, "documentary");
    assert_eq!(format.probabilities["listicle"], 0.12);
    assert_eq!(format.confidence.percent(), 76);

    let safe = decisions.yes_no("safe").unwrap();
    assert_eq!(safe.probability_yes, 0.95);
    assert_eq!(safe.confidence.percent(), 90);
}

#[test]
fn a_missing_answer_is_unexpected() {
    let failure = engine(&["systemone-missing-answer"])
        .decide(&key(), STATE, &questions())
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unexpected);
    assert!(failure.detail.contains("format"), "{}", failure.detail);
}

#[test]
fn a_rejected_key_fails_with_the_providers_message() {
    let failure = engine(&["systemone-rejected"])
        .decide(&key(), STATE, &questions())
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Rejected);
    assert!(failure.detail.starts_with("Cannot authenticate"));
}

#[test]
fn rate_limits_back_off_as_long_as_retry_after_asks() {
    let (sleeper, waits) = recording_sleeper();
    let answers = ["systemone-rate-limited", "systemone-answers"]
        .iter()
        .map(|name| fixture("jev", name))
        .collect();
    let jev = JevDecisionEngine::with_transport(Scripted::new(answers)).with_sleeper(sleeper);

    let decisions = jev.decide(&key(), STATE, &questions()).unwrap();

    assert!(decisions.score("fit").is_some());
    assert_eq!(jev.transport().sent().len(), 2);
    assert_eq!(*waits.lock().unwrap(), [Duration::from_secs(2)]);
}

#[test]
fn overload_backs_off_exponentially() {
    let (sleeper, waits) = recording_sleeper();
    let answers = [
        "systemone-overloaded",
        "systemone-overloaded",
        "systemone-overloaded",
        "systemone-answers",
    ]
    .iter()
    .map(|name| fixture("jev", name))
    .collect();
    let jev = JevDecisionEngine::with_transport(Scripted::new(answers)).with_sleeper(sleeper);

    jev.decide(&key(), STATE, &questions()).unwrap();

    assert_eq!(*waits.lock().unwrap(), [1, 2, 4].map(Duration::from_secs),);
}

#[test]
fn lasting_throttling_gives_up_with_the_right_kind() {
    let limited = engine(&["systemone-rate-limited"; 4])
        .decide(&key(), STATE, &questions())
        .unwrap_err();
    assert_eq!(limited.kind, ProviderFailureKind::LimitReached);
    assert_eq!(limited.detail, "Rate limit exceeded.");

    let overloaded = engine(&["systemone-overloaded"; 4])
        .decide(&key(), STATE, &questions())
        .unwrap_err();
    assert_eq!(overloaded.kind, ProviderFailureKind::ProviderDown);
}

#[test]
fn an_invalid_request_is_unexpected_and_names_the_field() {
    let failure = engine(&["systemone-invalid"])
        .decide(&key(), STATE, &questions())
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unexpected);
    assert!(failure.detail.contains("criteria"), "{}", failure.detail);
}

#[test]
fn no_answer_is_unreachable() {
    let jev = JevDecisionEngine::with_transport(Scripted::offline());
    let failure = jev.decide(&key(), STATE, &questions()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unreachable);
}

#[test]
fn no_questions_make_no_call() {
    let jev = engine(&[]);
    let decisions = jev.decide(&key(), STATE, &Questions::new()).unwrap();
    assert!(decisions.answers.is_empty());
    assert!(jev.transport().sent().is_empty());
}
