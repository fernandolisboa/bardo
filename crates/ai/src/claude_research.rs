//! Research on the web with Claude's own web search and web fetch tools
//! (ADR-0037).
//!
//! Anthropic runs the tools: the request names them with their limits and
//! Claude searches, fetches and writes within one answer. Bardo opens no
//! page itself. The limits of the research go with every request: the
//! most searches, the most pages and the most tokens read from a page.
//!
//! The answer is read as it streams, like any text. A research is a paid
//! call sent once (ADR-0030): an answer that is lost is not asked for
//! again. One thing does send again, by the API's own protocol: a turn
//! the API pauses (`stop_reason: "pause_turn"`, after ten steps of its
//! loop) is taken up by sending what it wrote back unchanged, once.
//!
//! The limits hold over the whole research, paused or not, whatever the
//! API counts a tool's uses by. The documentation says a use limit is for
//! "a turn" and not whether a turn taken up again starts its count anew,
//! so Bardo does the counting: the first request keeps one use of each
//! tool back, and the continuation is allowed what is left of the limit
//! after the searches and fetches already made. The room to write is
//! shared the same way.
//!
//! The request asks for the provider's prompt cache. Each search or fetch
//! starts another step in which Claude is given everything before it once
//! more; with the cache those tokens are billed as cache reads, a
//! twentieth of the price of sending them.
//!
//! What comes back is data: the notes are text for another request and
//! for the user to review, the pages are addresses and titles. Nothing a
//! page says is followed here.

use bardo_domain::{
    ApiKey, Metered, PageRead, Provider, ProviderFailure, ProviderFailureKind, ResearchLimits,
    ResearchNotes, ResearchRequest, Stop, WebResearcher,
};
use base64::Engine as _;
use serde_json::{Value, json};

use crate::claude::{
    Answered, EFFORT, MESSAGES_URL, MODEL, arrived_whole, bad_request, declined, refusal_is_billed,
    send_paid, unexpected,
};
use crate::claude_stream::StreamedAnswer;
use crate::http::{HttpRequest, Transport, UreqTransport};
use crate::idle::Watched;
use crate::key_check::failure;
use crate::retry::{Backoff, Sleeper, thread_sleeper};

/// The tool versions the request names: the current ones as of November
/// 2026, called by Claude directly.
pub const SEARCH_TOOL: &str = "web_search_20260318";
pub const FETCH_TOOL: &str = "web_fetch_20260318";
/// Room for the thought between steps and for the notes.
pub const MAX_TOKENS: u32 = 16_000;
/// The least room a continuation gets to finish the notes, however much
/// the paused answer wrote.
pub const CONTINUATION_MIN_TOKENS: u32 = 4_000;
/// The most a research writes, thought included: a first answer that used
/// all its room and a continuation at its least.
pub const OUTPUT_CEILING: u64 = MAX_TOKENS as u64 + CONTINUATION_MIN_TOKENS as u64;
/// How many times a paused turn is taken up again. The API pauses after
/// ten steps, and a research within its limits takes fourteen at most. A
/// turn that pauses a second time is given up.
pub const MAX_CONTINUATIONS: usize = 1;

/// The tools of a research, each with the uses the whole research may
/// make. Claude calls them directly (`allowed_callers`): the newer
/// default, running them from code Claude writes, would add a tool the
/// limits do not cover, and uses made from that code are not counted
/// against `max_uses` from one request of a turn to the next. A tool the
/// research has no use of is not offered.
///
/// Every request of a research sends these same tools. The API counts the
/// direct uses of a turn across its requests, so a continuation needs no
/// smaller limit, and a thought the paused answer holds stays valid only
/// while the tools it was written under are unchanged.
fn tools(limits: &ResearchLimits) -> Value {
    let mut tools = Vec::new();
    if limits.searches > 0 {
        tools.push(json!({
            "type": SEARCH_TOOL,
            "name": "web_search",
            "max_uses": limits.searches,
            "allowed_callers": ["direct"],
        }));
    }
    if limits.fetches > 0 {
        tools.push(json!({
            "type": FETCH_TOOL,
            "name": "web_fetch",
            "max_uses": limits.fetches,
            "max_content_tokens": limits.page_tokens,
            "allowed_callers": ["direct"],
        }));
    }
    Value::Array(tools)
}

/// The room a request gets to write in: the research's room, less what
/// the paused answers wrote, and never less than a continuation needs to
/// finish the notes.
fn room(written: u64) -> u32 {
    let left = u64::from(MAX_TOKENS).saturating_sub(written);
    u32::try_from(left)
        .unwrap_or(MAX_TOKENS)
        .max(CONTINUATION_MIN_TOKENS)
}

/// The Messages API request of a research. `paused` holds the content of
/// each answer the API paused, in order, exactly as it arrived: the turn
/// goes on from there, with room for `max_tokens`.
fn research_request(
    key: &ApiKey,
    request: &ResearchRequest,
    paused: &[Vec<Value>],
    max_tokens: u32,
) -> HttpRequest {
    // Pictures first, then the task about them.
    let mut content: Vec<Value> = request
        .images
        .iter()
        .map(|image| {
            json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": image.format.mime(),
                    "data": base64::engine::general_purpose::STANDARD.encode(&image.bytes),
                },
            })
        })
        .collect();
    content.push(json!({ "type": "text", "text": request.prompt }));
    let mut messages = vec![json!({ "role": "user", "content": content })];
    messages.extend(
        paused
            .iter()
            .map(|content| json!({ "role": "assistant", "content": content })),
    );
    let body = json!({
        "model": MODEL,
        "max_tokens": max_tokens,
        "stream": true,
        // What was read is kept in the provider's cache from one step to
        // the next, and from a paused answer to its continuation.
        "cache_control": { "type": "ephemeral" },
        "system": request.instructions,
        "tools": tools(&request.limits),
        "messages": messages,
        "output_config": { "effort": EFFORT },
    });
    HttpRequest::post_json(MESSAGES_URL, body.to_string())
        .header("x-api-key", key.expose())
        .header("anthropic-version", "2023-06-01")
}

/// What one answer used, in the meters it is billed by.
fn used(answer: &StreamedAnswer) -> Metered {
    Metered {
        input_tokens: answer.usage.input_tokens,
        output_tokens: answer.usage.output_tokens,
        cache_write_tokens: answer.extra.cache_write_tokens,
        cache_read_tokens: answer.extra.cache_read_tokens,
        searches: answer.extra.searches,
        ..Metered::default()
    }
}

fn sum(a: Metered, b: Metered) -> Metered {
    let add = u64::saturating_add;
    Metered {
        input_tokens: add(a.input_tokens, b.input_tokens),
        output_tokens: add(a.output_tokens, b.output_tokens),
        cache_write_tokens: add(a.cache_write_tokens, b.cache_write_tokens),
        cache_read_tokens: add(a.cache_read_tokens, b.cache_read_tokens),
        searches: add(a.searches, b.searches),
        ..a
    }
}

fn is_tool_block(block: &Value) -> bool {
    block["type"].as_str().is_some_and(|kind| {
        kind == "server_tool_use" || kind.ends_with("_tool_result") || kind == "tool_use"
    })
}

/// The notes: the text written after the last search or fetch. What
/// Claude says between its steps ("let me look at the pricing page") is
/// not part of them.
fn notes_text(content: &[Value]) -> String {
    let after_tools = content
        .iter()
        .rposition(is_tool_block)
        .map_or(0, |last| last + 1);
    content[after_tools..]
        .iter()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block["text"].as_str())
        .collect::<String>()
        .trim()
        .to_owned()
}

/// The pages a research read: every page it fetched, and every search
/// result its notes cite or name by its address, each once, in the order
/// they were read.
fn pages_read(content: &[Value], notes: &str) -> Vec<PageRead> {
    let cited: Vec<&str> = content
        .iter()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block["citations"].as_array())
        .flatten()
        .filter_map(|citation| citation["url"].as_str())
        .collect();
    let mut pages: Vec<PageRead> = Vec::new();
    let mut add = |page: PageRead| {
        if page.url.is_empty() {
            return;
        }
        match pages.iter_mut().find(|known| known.url == page.url) {
            // A page first seen in a search and then fetched has a time.
            Some(known) => {
                if known.read_at.is_none() {
                    known.read_at = page.read_at;
                }
                if known.title.is_empty() {
                    known.title = page.title;
                }
            }
            None => pages.push(page),
        }
    };
    for block in content {
        let result = &block["content"];
        match block["type"].as_str() {
            Some("web_fetch_tool_result") if result["type"] == "web_fetch_result" => {
                add(PageRead {
                    url: result["url"].as_str().unwrap_or_default().to_owned(),
                    title: one_line(result["content"]["title"].as_str().unwrap_or_default()),
                    read_at: result["retrieved_at"]
                        .as_str()
                        .and_then(|at| humantime::parse_rfc3339_weak(at).ok()),
                });
            }
            // A failed search holds an error object in the place of the
            // list.
            Some("web_search_tool_result") => {
                for found in result.as_array().into_iter().flatten() {
                    let url = found["url"].as_str().unwrap_or_default();
                    if found["type"] == "web_search_result"
                        && (cited.contains(&url) || (!url.is_empty() && notes.contains(url)))
                    {
                        add(PageRead {
                            url: url.to_owned(),
                            title: one_line(found["title"].as_str().unwrap_or_default()),
                            read_at: None,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    pages
}

/// A page's title on one line, cut where a title has long ended.
fn one_line(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    line.chars().take(200).collect()
}

/// Researches with Claude over HTTPS.
pub struct ClaudeWebResearcher<T = Watched<UreqTransport>> {
    transport: T,
    backoff: Backoff,
    sleep: Sleeper,
}

impl ClaudeWebResearcher {
    pub fn new() -> Self {
        use crate::claude::ClaudeTextGenerator as Text;
        Self::with_transport(Watched::new(
            UreqTransport::for_streams(Text::CONNECT, Text::FIRST_ANSWER, Text::WHOLE_ANSWER),
            Text::SILENCE,
        ))
    }
}

impl Default for ClaudeWebResearcher {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transport> ClaudeWebResearcher<T> {
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

/// The most a research within `limits` is billed, `sent` tokens of
/// instructions, prompt and pictures in.
///
/// The request asks for the provider's cache, so what the model holds is
/// written to it once and read from it at every later step. A turn that is
/// taken up again is a new request: it sends the same tools and the same
/// start, so it should read the cache, but the ceiling does not count on
/// it and counts everything written once more for each continuation.
/// What is sent is counted as plain input too, the part of a request the
/// cache does not cover.
fn ceiling(limits: &ResearchLimits, sent: u64) -> Metered {
    let held = sent.saturating_add(limits.read_ceiling());
    Metered {
        input_tokens: sent,
        output_tokens: OUTPUT_CEILING,
        searches: u64::from(limits.searches),
        cache_write_tokens: held.saturating_mul(1 + MAX_CONTINUATIONS as u64),
        cache_read_tokens: limits.reread_ceiling(sent),
        ..Metered::default()
    }
}

impl<T: Transport> WebResearcher for ClaudeWebResearcher<T> {
    fn ceiling(&self, limits: &ResearchLimits, sent: u64) -> Metered {
        ceiling(limits, sent)
    }

    fn research_until(
        &self,
        key: &ApiKey,
        request: &ResearchRequest,
        stop: &Stop,
    ) -> Result<ResearchNotes, ProviderFailure> {
        // The answers the API paused, and what they used.
        let mut paused: Vec<Vec<Value>> = Vec::new();
        let mut usage = Metered::default();
        let mut model = MODEL.to_owned();
        // A failure after an answer was paid for says what was used so
        // far. A lost answer says nothing: Costs then counts what a whole
        // research usually uses, which covers the part that was lost.
        let so_far = |failure: ProviderFailure, model: &str, usage: Metered| {
            if failure.billed.is_none()
                && failure.kind != ProviderFailureKind::AnswerLost
                && usage != Metered::default()
            {
                failure.billed(model, usage)
            } else {
                failure
            }
        };
        let answer = loop {
            let max_tokens = if paused.is_empty() {
                MAX_TOKENS
            } else {
                room(usage.output_tokens)
            };
            let http = research_request(key, request, &paused, max_tokens);
            let answered = send_paid(&self.transport, &self.backoff, &self.sleep, &http, stop)
                .map_err(|failure| so_far(failure, &model, usage))?;
            // An empty balance is reported as a bad request.
            if let Some(message) = bad_request(&answered)
                && message.contains("credit balance")
            {
                return Err(so_far(
                    ProviderFailure::new(ProviderFailureKind::LimitReached, message),
                    &model,
                    usage,
                ));
            }
            let answer = match answered {
                Answered::Answer(answer) => *answer,
                Answered::Refused(response) => {
                    return Err(so_far(failure(Provider::Claude, &response), &model, usage));
                }
            };
            arrived_whole(&answer).map_err(|failure| so_far(failure, &model, usage))?;
            if let Some(answered_by) = answer.model.as_deref().filter(|model| !model.is_empty()) {
                model = answered_by.to_owned();
            }
            let before = usage;
            usage = sum(usage, used(&answer));
            let failed = |failure: ProviderFailure| failure.billed(model.clone(), usage);
            match answer.stop_reason.as_deref() {
                Some("pause_turn") if paused.len() < MAX_CONTINUATIONS => {
                    paused.push(answer.content);
                }
                Some("pause_turn") => {
                    return Err(failed(unexpected(
                        "the research was paused again after Bardo took it up",
                    )));
                }
                Some("refusal") => {
                    let declined = declined(&answer);
                    return Err(if refusal_is_billed(&answer) {
                        failed(declined)
                    } else {
                        so_far(declined, &model, before)
                    });
                }
                Some("max_tokens") => {
                    return Err(failed(unexpected(
                        "the research was cut at the token limit before its notes were whole",
                    )));
                }
                Some("model_context_window_exceeded") => {
                    return Err(failed(unexpected(
                        "the research was cut at the end of the model's context window",
                    )));
                }
                // Nothing a message says: what Claude may have written
                // did not arrive.
                None if answer.content.is_empty() => {
                    return Err(ProviderFailure::new(
                        ProviderFailureKind::AnswerLost,
                        "Claude took the request and what came back is not an answer, so it may have researched and billed one",
                    ));
                }
                _ => break answer,
            }
        };
        let content: Vec<Value> = paused.into_iter().flatten().chain(answer.content).collect();
        let text = notes_text(&content);
        if text.is_empty() {
            return Err(unexpected("the research wrote no notes").billed(model, usage));
        }
        let pages = pages_read(&content, &text);
        Ok(ResearchNotes {
            text,
            pages,
            model,
            usage,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(text: &str) -> Value {
        json!({ "type": "text", "text": text })
    }

    fn search(id: &str, results: &[(&str, &str)]) -> [Value; 2] {
        let results: Vec<Value> = results
            .iter()
            .map(|(url, title)| {
                json!({ "type": "web_search_result", "url": url, "title": title,
                        "encrypted_content": "Eq" })
            })
            .collect();
        [
            json!({ "type": "server_tool_use", "id": id, "name": "web_search",
                    "input": { "query": "desk lamp reviews" } }),
            json!({ "type": "web_search_tool_result", "tool_use_id": id, "content": results }),
        ]
    }

    fn fetch(id: &str, url: &str, title: &str) -> [Value; 2] {
        [
            json!({ "type": "server_tool_use", "id": id, "name": "web_fetch",
                    "input": { "url": url } }),
            json!({ "type": "web_fetch_tool_result", "tool_use_id": id, "content": {
                "type": "web_fetch_result", "url": url, "retrieved_at": "2026-11-20T10:30:00Z",
                "content": { "type": "document", "title": title,
                             "source": { "type": "text", "media_type": "text/plain", "data": "…" } },
            }}),
        ]
    }

    #[test]
    fn the_tools_carry_the_limits_of_the_whole_research_and_leave_out_a_tool_with_none() {
        let limits = ResearchLimits::PRODUCT;
        let all = tools(&limits);
        assert_eq!(all[0]["max_uses"], limits.searches);
        assert_eq!(all[1]["max_uses"], limits.fetches);
        assert_eq!(all[1]["max_content_tokens"], limits.page_tokens);
        let only_fetches = ResearchLimits {
            searches: 0,
            fetches: 1,
            page_tokens: 4_000,
        };
        let one = tools(&only_fetches);
        assert_eq!(one.as_array().unwrap().len(), 1);
        assert_eq!(one[0]["name"], "web_fetch");
    }

    #[test]
    fn a_continuation_writes_in_what_is_left_of_the_room_and_never_in_too_little() {
        assert_eq!(room(0), MAX_TOKENS);
        assert_eq!(room(5_000), 11_000);
        assert_eq!(room(15_000), CONTINUATION_MIN_TOKENS);
        assert_eq!(room(90_000), CONTINUATION_MIN_TOKENS);
        assert_eq!(OUTPUT_CEILING, 20_000);
    }

    #[test]
    fn the_ceiling_counts_the_cache_written_twice_and_every_step_read_again() {
        let limits = ResearchLimits::PRODUCT;
        let most = ceiling(&limits, 3_000);
        assert_eq!(most.input_tokens, 3_000);
        assert_eq!(most.output_tokens, 20_000);
        assert_eq!(most.searches, 8);
        // Everything held, written once and once more after a pause.
        assert_eq!(most.cache_write_tokens, 2 * (3_000 + 148_000));
        assert_eq!(most.cache_read_tokens, limits.reread_ceiling(3_000));
        assert_eq!(most.cache_read_tokens, 1_355_000);
        assert_eq!(most.image_tokens, 0);
    }

    #[test]
    fn the_notes_are_what_was_written_after_the_last_tool() {
        let [call, result] = fetch("srvtoolu_1", "https://lamp.example/", "Desk lamp");
        let content = [
            text("Let me read the product page."),
            call,
            result,
            text("## Notes\n\nThe lamp "),
            text("lights a 160 cm desk.\n"),
        ];
        assert_eq!(
            notes_text(&content),
            "## Notes\n\nThe lamp lights a 160 cm desk."
        );
        // Without a tool, all of the text is the notes.
        assert_eq!(notes_text(&[text("Nothing to read.")]), "Nothing to read.");
        // A turn that ends on a tool wrote none.
        let [call, result] = fetch("srvtoolu_1", "https://lamp.example/", "Desk lamp");
        assert_eq!(notes_text(&[text("Reading."), call, result]), "");
    }

    #[test]
    fn pages_are_the_fetched_ones_and_the_search_results_the_notes_use() {
        let [search_call, search_result] = search(
            "srvtoolu_1",
            &[
                ("https://reviews.example/lamps", "Lamps  compared\n2026"),
                ("https://shop.example/lamp", "Buy the lamp"),
                ("https://forum.example/t/9", "Is it any good?"),
            ],
        );
        let [fetch_call, fetch_result] = fetch("srvtoolu_2", "https://lamp.example/", "Desk lamp");
        let mut cited = text("Reviewers rate it best for long days.");
        cited["citations"] = json!([{
            "type": "web_search_result_location",
            "url": "https://reviews.example/lamps",
            "title": "Lamps compared",
            "cited_text": "best for long days",
        }]);
        let notes = "It sells for US$ 89 (https://shop.example/lamp).";
        let content = [
            search_call,
            search_result,
            fetch_call,
            fetch_result,
            cited,
            text(notes),
        ];
        let pages = pages_read(&content, notes);
        let urls: Vec<&str> = pages.iter().map(|page| page.url.as_str()).collect();
        assert_eq!(
            urls,
            [
                "https://reviews.example/lamps",
                "https://shop.example/lamp",
                "https://lamp.example/"
            ],
            "the forum page was found and not used"
        );
        assert_eq!(pages[0].title, "Lamps compared 2026");
        assert_eq!(pages[0].read_at, None);
        assert_eq!(
            pages[2].read_at,
            Some(humantime::parse_rfc3339("2026-11-20T10:30:00Z").unwrap())
        );
    }

    #[test]
    fn a_page_found_and_then_fetched_is_listed_once_with_its_time() {
        let [search_call, search_result] =
            search("srvtoolu_1", &[("https://lamp.example/", "Desk lamp")]);
        let [fetch_call, fetch_result] = fetch("srvtoolu_2", "https://lamp.example/", "");
        let notes = "From https://lamp.example/ itself.";
        let content = [
            search_call,
            search_result,
            fetch_call,
            fetch_result,
            text(notes),
        ];
        let pages = pages_read(&content, notes);
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].title, "Desk lamp");
        assert!(pages[0].read_at.is_some());
    }

    #[test]
    fn a_failed_search_or_fetch_reads_no_page() {
        let content = [
            json!({ "type": "web_search_tool_result", "tool_use_id": "srvtoolu_1",
                    "content": { "type": "web_search_tool_result_error",
                                 "error_code": "too_many_requests" } }),
            json!({ "type": "web_fetch_tool_result", "tool_use_id": "srvtoolu_2",
                    "content": { "type": "web_fetch_tool_result_error",
                                 "error_code": "url_not_accessible" } }),
            text("Nothing could be read."),
        ];
        assert!(pages_read(&content, "Nothing could be read.").is_empty());
    }
}
