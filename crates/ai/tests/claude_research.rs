//! Research on the web with Claude against recorded responses (see
//! `fixtures/claude/README.md`). No test calls Claude.

mod common;

use bardo_ai::ClaudeWebResearcher;
use bardo_ai::claude::{MESSAGES_URL, MODEL};
use bardo_ai::claude_research::{FETCH_TOOL, MAX_CONTINUATIONS, SEARCH_TOOL};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bardo_ai::http::{HttpRequest, HttpResponse, StreamedResponse, Transport, TransportError};
use bardo_domain::{
    ApiKey, BilledUse, ImageFormat, Metered, Provider, ProviderFailureKind, ResearchLimits,
    ResearchRequest, Stop, TextImage, WebResearcher,
};
use common::{Scripted, fixture, recording_sleeper};
use serde_json::{Value, json};

const KEY: &str = "sk-ant-api03-test-key-0001";
const TASK: &str = "Research the Lumen Arc desk lamp. Its page: https://lumenarc.example/";

fn key() -> ApiKey {
    ApiKey::parse(Provider::Claude, KEY).unwrap()
}

fn request() -> ResearchRequest {
    ResearchRequest {
        instructions: "You research a product before an ad is written about it.".into(),
        prompt: TASK.into(),
        images: Vec::new(),
        limits: ResearchLimits::PRODUCT,
    }
}

fn researcher(answers: &[&str]) -> ClaudeWebResearcher<Scripted> {
    let answers = answers.iter().map(|name| fixture("claude", name)).collect();
    let (sleeper, _) = recording_sleeper();
    ClaudeWebResearcher::with_transport(Scripted::new(answers)).with_sleeper(sleeper)
}

fn types(content: &Value) -> Vec<&str> {
    content
        .as_array()
        .unwrap()
        .iter()
        .map(|block| block["type"].as_str().unwrap())
        .collect()
}

#[test]
fn the_request_carries_the_tools_with_the_limits_of_the_research() {
    let claude = researcher(&["research-whole"]);
    claude.research(&key(), &request()).unwrap();

    let sent = claude.transport().sent();
    assert_eq!(sent.len(), 1);
    let sent = &sent[0];
    assert_eq!(sent.url, MESSAGES_URL);
    assert_eq!(sent.header("x-api-key"), Some(KEY));
    assert_eq!(sent.header("anthropic-version"), Some("2023-06-01"));
    assert_eq!(sent.header("anthropic-beta"), None);
    assert_eq!(sent.body["model"], MODEL);
    assert_eq!(sent.body["stream"], true);
    assert_eq!(
        sent.body["system"],
        "You research a product before an ad is written about it."
    );
    assert_eq!(
        sent.body["messages"],
        json!([{ "role": "user", "content": [{ "type": "text", "text": TASK }] }])
    );
    // The current versions of both tools, called by Claude itself, each
    // with the limit of the whole research: the API counts the uses of a
    // turn across its requests.
    assert_eq!(SEARCH_TOOL, "web_search_20260318");
    assert_eq!(FETCH_TOOL, "web_fetch_20260318");
    assert_eq!(
        sent.body["tools"],
        json!([
            {
                "type": "web_search_20260318",
                "name": "web_search",
                "max_uses": 8,
                "allowed_callers": ["direct"],
            },
            {
                "type": "web_fetch_20260318",
                "name": "web_fetch",
                "max_uses": 5,
                "max_content_tokens": 20_000,
                "allowed_callers": ["direct"],
            },
        ])
    );
    assert_eq!(sent.body["max_tokens"], 16_000);
    // What is read is cached from one step to the next.
    assert_eq!(sent.body["cache_control"], json!({ "type": "ephemeral" }));
    assert!(sent.body.get("fallbacks").is_none());
    assert!(sent.body.get("thinking").is_none(), "the model decides");
}

#[test]
fn other_limits_are_sent_as_given() {
    let claude = researcher(&["research-whole"]);
    let narrow = ResearchRequest {
        limits: ResearchLimits {
            searches: 3,
            fetches: 1,
            page_tokens: 4_000,
        },
        ..request()
    };
    claude.research(&key(), &narrow).unwrap();
    let tools = &claude.transport().sent()[0].body["tools"];
    assert_eq!(tools[0]["max_uses"], 3);
    assert_eq!(tools[1]["max_uses"], 1);
    assert_eq!(tools[1]["max_content_tokens"], 4_000);

    // A research with no searches is not offered the search tool.
    let claude = researcher(&["research-whole"]);
    let page_only = ResearchRequest {
        limits: ResearchLimits {
            searches: 0,
            fetches: 2,
            page_tokens: 4_000,
        },
        ..request()
    };
    claude.research(&key(), &page_only).unwrap();
    let tools = claude.transport().sent()[0].body["tools"].clone();
    assert_eq!(tools.as_array().unwrap().len(), 1);
    assert_eq!(tools[0]["name"], "web_fetch");
}

#[test]
fn pictures_go_before_the_task() {
    let claude = researcher(&["research-whole"]);
    let mut asked = request();
    asked.images = vec![TextImage {
        bytes: vec![0xFF, 0xD8, 0xFF, 0xE0],
        format: ImageFormat::Jpeg,
    }];
    claude.research(&key(), &asked).unwrap();
    assert_eq!(
        claude.transport().sent()[0].body["messages"][0]["content"],
        json!([
            {
                "type": "image",
                "source": { "type": "base64", "media_type": "image/jpeg", "data": "/9j/4A==" },
            },
            { "type": "text", "text": TASK },
        ])
    );
}

#[test]
fn a_whole_research_gives_its_notes_its_pages_and_all_it_used() {
    let claude = researcher(&["research-whole"]);
    let notes = claude.research(&key(), &request()).unwrap();

    // The notes are what came after the last tool, without what Claude
    // said on its way.
    assert!(
        notes
            .text
            .starts_with("## Product\n\nThe Lumen Arc is a desk lamp")
    );
    assert!(
        notes
            .text
            .ends_with("lights the keyboard and not the desk.")
    );
    assert!(!notes.text.contains("I'll read the product page first."));
    assert_eq!(notes.model, "claude-opus-5-5");

    // The page it fetched, and of the three search results the one the
    // notes name and the one they cite; the forum thread was not used.
    let pages: Vec<(&str, &str, bool)> = notes
        .pages
        .iter()
        .map(|page| {
            (
                page.url.as_str(),
                page.title.as_str(),
                page.read_at.is_some(),
            )
        })
        .collect();
    assert_eq!(
        pages,
        [
            ("https://lumenarc.example/", "Lumen Arc desk lamp", true),
            (
                "https://deskreview.example/best-desk-lamps-2026",
                "Best desk lamps of 2026, tested",
                false
            ),
            (
                "https://homeoffice.example/lighting/monitor-bars-vs-lamps",
                "Monitor light bars vs desk lamps",
                false
            ),
        ]
    );

    // Every step of the answer is counted, with the tokens that went
    // through the cache and the search the provider ran. A fetch is not
    // billed beyond its tokens.
    assert_eq!(
        notes.usage,
        Metered {
            input_tokens: 2_140 + 14 + 12,
            output_tokens: 180 + 210 + 1_450,
            cache_write_tokens: 14_120 + 5_560,
            cache_read_tokens: 14_120,
            searches: 1,
            ..Metered::default()
        }
    );
}

#[test]
fn a_paused_turn_is_taken_up_with_what_it_wrote_sent_back_unchanged() {
    let claude = researcher(&["research-paused", "research-resumed"]);
    let notes = claude.research(&key(), &request()).unwrap();

    let sent = claude.transport().sent();
    assert_eq!(sent.len(), 2);
    // The continuation is the same request with the paused answer after
    // the task.
    for field in ["model", "system", "tools", "cache_control", "output_config"] {
        assert_eq!(sent[1].body[field], sent[0].body[field], "{field}");
    }
    // The tools are the first request's, to the letter: the API counts
    // the uses of the turn across both requests, and the thought the
    // paused answer holds is valid only under the tools it was written
    // with.
    assert_eq!(
        sent[1].body["tools"].to_string(),
        sent[0].body["tools"].to_string()
    );
    assert_eq!(sent[1].body["tools"][0]["max_uses"], 8);
    assert_eq!(sent[1].body["tools"][1]["max_uses"], 5);
    // It writes in the room the first answer left.
    assert_eq!(sent[1].body["max_tokens"], 16_000 - 150 - 120);
    let messages = sent[1].body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0], sent[0].body["messages"][0]);
    assert_eq!(messages[1]["role"], "assistant");
    let content = &messages[1]["content"];
    assert_eq!(
        types(content),
        [
            "text",
            "server_tool_use",
            "web_search_tool_result",
            "thinking",
            "server_tool_use"
        ]
    );
    // Each block as a whole message holds it: the tool's input put
    // together, the results with what the API needs to read them again,
    // the thought with its signature, and the fetch that had not run.
    assert_eq!(
        content[0],
        json!({ "type": "text", "text": "I'll search first." })
    );
    assert_eq!(
        content[1],
        json!({
            "type": "server_tool_use",
            "id": "srvtoolu_01SearchFixture0000001",
            "name": "web_search",
            "input": { "query": "Lumen Arc desk lamp review" },
        })
    );
    assert_eq!(content[2]["content"].as_array().unwrap().len(), 3);
    assert_eq!(
        content[2]["content"][0]["encrypted_content"],
        "EqgfCioIARgBIiQ3YTAwMjY1fixture0"
    );
    assert_eq!(
        content[3],
        json!({ "type": "thinking", "thinking": "", "signature": "EqQBCkYIBxgCKkA0paused1" })
    );
    assert_eq!(
        content[4]["input"],
        json!({ "url": "https://lumenarc.example/" })
    );

    // The research is both answers: the page fetched in the second, the
    // search result of the first that the notes name.
    let urls: Vec<&str> = notes.pages.iter().map(|page| page.url.as_str()).collect();
    assert_eq!(
        urls,
        [
            "https://deskreview.example/best-desk-lamps-2026",
            "https://lumenarc.example/"
        ]
    );
    assert!(notes.text.starts_with("## Product"));
    assert_eq!(
        notes.usage,
        Metered {
            input_tokens: 2_140 + 12 + 9,
            output_tokens: 150 + 120 + 1_380,
            cache_write_tokens: 5_560 + 14_120,
            cache_read_tokens: 7_700,
            searches: 1,
            ..Metered::default()
        }
    );
}

#[test]
fn a_turn_that_pauses_again_is_given_up_with_what_it_used() {
    let claude = researcher(&["research-paused", "research-paused-again"]);
    let failure = claude.research(&key(), &request()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unexpected);
    assert_eq!(claude.transport().sent().len(), 1 + MAX_CONTINUATIONS);
    // Every paused answer is kept, in order.
    let last = &claude.transport().sent()[MAX_CONTINUATIONS].body["messages"];
    assert_eq!(last.as_array().unwrap().len(), 1 + MAX_CONTINUATIONS);
    assert_eq!(
        failure.billed.as_deref(),
        Some(&BilledUse {
            model: "claude-opus-5-5".into(),
            usage: Metered {
                input_tokens: 2_140 + 12 + 9,
                output_tokens: 150 + 120 + 40,
                cache_write_tokens: 5_560 + 14_120,
                cache_read_tokens: 7_700,
                searches: 1,
                ..Metered::default()
            },
        })
    );
}

#[test]
fn a_tool_error_inside_the_answer_is_not_a_failure() {
    // The fetch of the product page fails and a search is refused for
    // passing its limit, both inside a successful answer: Claude goes on
    // and the notes say what could not be read.
    let claude = researcher(&["research-tool-error"]);
    let notes = claude.research(&key(), &request()).unwrap();
    assert!(notes.text.contains("could not be read"));
    let urls: Vec<&str> = notes.pages.iter().map(|page| page.url.as_str()).collect();
    assert_eq!(urls, ["https://deskreview.example/best-desk-lamps-2026"]);
    // The provider counts the search it ran, not the one it refused.
    assert_eq!(notes.usage.searches, 1);
    assert_eq!(notes.usage.cache_read_tokens, 2_200 + 7_760);
}

#[test]
fn a_refusal_midway_is_a_failure_billed_for_what_it_read() {
    let claude = researcher(&["research-refusal"]);
    let failure = claude.research(&key(), &request()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Declined);
    assert!(failure.detail.contains("does not write"));
    assert_eq!(
        failure.billed.as_deref(),
        Some(&BilledUse {
            model: "claude-opus-5-5".into(),
            usage: Metered {
                input_tokens: 2_140 + 12,
                output_tokens: 60,
                cache_write_tokens: 5_560,
                searches: 1,
                ..Metered::default()
            },
        })
    );
    assert_eq!(claude.transport().sent().len(), 1, "not asked again");
}

#[test]
fn a_refusal_before_anything_was_written_bills_nothing() {
    let claude = researcher(&["messages-refusal"]);
    let failure = claude.research(&key(), &request()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Declined);
    assert_eq!(failure.billed, None);
}

#[test]
fn a_research_cut_at_the_token_limit_is_billed_and_lost() {
    let claude = researcher(&["research-max-tokens"]);
    let failure = claude.research(&key(), &request()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unexpected);
    let billed = failure.billed.unwrap();
    assert_eq!(billed.usage.output_tokens, 60 + 31_940);
    assert_eq!(billed.usage.searches, 1);
}

#[test]
fn a_lost_answer_is_never_asked_for_again() {
    // The stream stops inside the answer.
    let claude = researcher(&["messages-dropped-midway"]);
    let failure = claude.research(&key(), &request()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::AnswerLost);
    assert_eq!(failure.billed, None);
    assert_eq!(claude.transport().sent().len(), 1);

    // The request left and nothing came back.
    let (sleeper, _) = recording_sleeper();
    let claude =
        ClaudeWebResearcher::with_transport(Scripted::losing_the_answer()).with_sleeper(sleeper);
    let failure = claude.research(&key(), &request()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::AnswerLost);
    assert_eq!(claude.transport().sent().len(), 1);
}

#[test]
fn a_continuation_whose_answer_is_lost_is_a_lost_answer() {
    // The first answer was paid for; Costs counts a whole research for a
    // lost one, so the failure carries no usage of its own.
    let (sleeper, _) = recording_sleeper();
    let claude = ClaudeWebResearcher::with_transport(Scripted::with_answers(vec![
        Ok(fixture("claude", "research-paused")),
        Err(TransportError::unanswered("timeout: global")),
    ]))
    .with_sleeper(sleeper);
    let failure = claude.research(&key(), &request()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::AnswerLost);
    assert_eq!(failure.billed, None);
    assert_eq!(claude.transport().sent().len(), 2);
}

#[test]
fn a_continuation_that_is_refused_says_what_the_first_answer_used() {
    let claude = researcher(&["research-paused", "messages-rejected"]);
    let failure = claude.research(&key(), &request()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Rejected);
    assert_eq!(
        failure.billed.map(|billed| billed.usage),
        Some(Metered {
            input_tokens: 2_140 + 12,
            output_tokens: 150 + 120,
            cache_write_tokens: 5_560,
            searches: 1,
            ..Metered::default()
        })
    );
}

/// Sets `answered` once an answer came back: a job cancelled between the
/// requests of a research.
struct StoppedAfterOne {
    scripted: Scripted,
    answered: Arc<AtomicBool>,
}

impl Transport for StoppedAfterOne {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError> {
        self.scripted.send(request)
    }

    fn send_streamed_until(
        &self,
        request: &HttpRequest,
        stop: &Stop,
    ) -> Result<StreamedResponse, TransportError> {
        let response = self.scripted.send_streamed_until(request, stop);
        self.answered.store(true, Ordering::SeqCst);
        response
    }
}

#[test]
fn a_research_that_is_stopped_sends_nothing_more() {
    // Stopped before it left: nothing is sent and nothing is billed.
    let claude = researcher(&["research-whole"]);
    let failure = claude
        .research_until(&key(), &request(), &Stop::when(|| true))
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unreachable);
    assert_eq!(failure.billed, None);
    assert!(claude.transport().sent().is_empty());

    // Stopped after the API paused it: the continuation is not sent, and
    // the failure says what the paused answer used.
    let answered = Arc::<AtomicBool>::default();
    let (sleeper, _) = recording_sleeper();
    let claude = ClaudeWebResearcher::with_transport(StoppedAfterOne {
        scripted: Scripted::new(vec![
            fixture("claude", "research-paused"),
            fixture("claude", "research-resumed"),
        ]),
        answered: Arc::clone(&answered),
    })
    .with_sleeper(sleeper);
    let stop = Stop::when(move || answered.load(Ordering::SeqCst));
    let failure = claude
        .research_until(&key(), &request(), &stop)
        .unwrap_err();
    assert_eq!(claude.transport().scripted.sent().len(), 1);
    assert_eq!(failure.kind, ProviderFailureKind::Unreachable);
    assert_eq!(
        failure.billed.map(|billed| billed.usage),
        Some(Metered {
            input_tokens: 2_140 + 12,
            output_tokens: 150 + 120,
            cache_write_tokens: 5_560,
            searches: 1,
            ..Metered::default()
        })
    );
}

#[test]
fn an_overloaded_api_is_asked_again_and_a_bad_key_is_not() {
    let answers = ["messages-overloaded", "research-whole"]
        .iter()
        .map(|name| fixture("claude", name))
        .collect();
    let (sleeper, waits) = recording_sleeper();
    let claude = ClaudeWebResearcher::with_transport(Scripted::new(answers)).with_sleeper(sleeper);
    claude.research(&key(), &request()).unwrap();
    assert_eq!(claude.transport().sent().len(), 2);
    assert_eq!(waits.lock().unwrap().len(), 1);

    let claude = researcher(&["messages-rejected"]);
    let failure = claude.research(&key(), &request()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Rejected);
    assert_eq!(failure.billed, None);
    assert_eq!(claude.transport().sent().len(), 1);

    let claude = researcher(&["messages-credit-balance"]);
    let failure = claude.research(&key(), &request()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::LimitReached);
}

#[test]
fn a_research_without_notes_is_a_failure_that_says_what_it_used() {
    let claude = researcher(&["messages-no-text"]);
    let failure = claude.research(&key(), &request()).unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unexpected);
    assert!(failure.billed.is_some());
}
