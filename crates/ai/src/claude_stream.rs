//! Reads a Messages API answer as Claude streams it (server-sent events).
//!
//! A streamed answer is never cut by a clock on the whole of it, and when
//! it stops early what arrived is known: the model, the input it was billed
//! and the text so far. The reader therefore never fails; it reports how
//! far the answer got and how it ended, and the adapter decides what that
//! means.
//!
//! Besides the text, the reader rebuilds the answer's content blocks as a
//! whole message would hold them (thought with its signature, a tool's
//! input, a tool's result, the pages a text cites): a turn the API paused
//! is taken up again by sending those blocks back unchanged.

use std::io::Read;

use bardo_domain::TokenUsage;
use serde_json::Value;

use crate::sse::Events;

/// A stream larger than this is not an answer: 128,000 output tokens in
/// the smallest events the API sends stay well below it.
pub const MAX_STREAM_BYTES: u64 = 64 * 1024 * 1024;

/// How a stream ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEnd {
    /// The answer is whole, whatever its stop reason: `message_stop`
    /// arrived, or the stream ended after the message gave its stop reason,
    /// which comes after all of its content.
    Complete,
    /// The API ended the stream with an `error` event, e.g.
    /// `overloaded_error`.
    Failed { kind: String, message: String },
    /// The stream stopped before the answer did: the connection dropped,
    /// went silent, or sent something unreadable.
    Broken(String),
}

/// What a streamed answer said, as far as it got.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamedAnswer {
    /// The model that answers, from `message_start`. `None` when no
    /// message start was read.
    pub model: Option<String>,
    /// Whether anything of an answer arrived: a message started, or a
    /// part of one came although its start could not be read.
    pub began: bool,
    /// The text blocks so far, in order.
    pub text: String,
    pub stop_reason: Option<String>,
    /// Why a request was declined, when the API says.
    pub stop_details: Value,
    /// As the API counted them so far, from `message_start` and then from
    /// each `message_delta`. When more than one model worked on the answer
    /// (a fallback), the sum of what each used.
    pub usage: TokenUsage,
    /// What the answer used beside those tokens.
    pub extra: ExtraUsage,
    /// The content blocks so far, in order, each as a message that was
    /// not streamed would hold it.
    pub content: Vec<Value>,
    pub end: StreamEnd,
}

/// What an answer used beside the tokens sent and written: the tokens that
/// went through the provider's prompt cache, which `usage` leaves out, and
/// the tools the provider ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExtraUsage {
    /// Tokens sent that were written to the cache.
    pub cache_write_tokens: u64,
    /// Tokens sent that were read from the cache.
    pub cache_read_tokens: u64,
    /// Web searches the provider ran.
    pub searches: u64,
    /// Pages the provider fetched.
    pub fetches: u64,
}

impl StreamedAnswer {
    /// An answer of which nothing arrived, ended as `end` says.
    pub fn unstarted(end: StreamEnd) -> Self {
        Self {
            model: None,
            began: false,
            text: String::new(),
            stop_reason: None,
            stop_details: Value::Null,
            usage: TokenUsage::default(),
            extra: ExtraUsage::default(),
            content: Vec::new(),
            end,
        }
    }

    /// Whether generation began, so the request is billed even though the
    /// answer may not have arrived whole.
    pub fn started(&self) -> bool {
        self.began
    }
}

/// The events that carry something to read. Keep-alives (`ping`) and event
/// types added later do not.
fn is_read(kind: &str) -> bool {
    matches!(
        kind,
        "message_start"
            | "content_block_start"
            | "content_block_delta"
            | "content_block_stop"
            | "message_delta"
            | "message_stop"
            | "error"
    )
}

/// Reads the events of `body` until the message stops, the API reports an
/// error, or the stream breaks.
pub fn read(body: impl Read) -> StreamedAnswer {
    let mut answer = StreamedAnswer::unstarted(StreamEnd::Complete);
    let mut stopped = false;
    answer.end = read_into(&mut answer, &mut stopped, body);
    // The stop reason is the last thing a message says, after all of its
    // content: an answer that gave one is whole, whatever happened to the
    // stream after it.
    if stopped {
        answer.end = StreamEnd::Complete;
    }
    answer
}

/// Reads events into `answer` and says how the stream ended. `stopped`
/// says whether the last the message said was its stop reason.
fn read_into(answer: &mut StreamedAnswer, stopped: &mut bool, body: impl Read) -> StreamEnd {
    // A tool's input arrives as pieces of JSON, by the block's place.
    let mut inputs: Vec<(usize, String)> = Vec::new();
    for event in Events::new(body.take(MAX_STREAM_BYTES)) {
        let event = match event {
            Ok(event) => event,
            Err(error) => return StreamEnd::Broken(error.to_string()),
        };
        // Every event says its type twice, in its name and in its data. A
        // stream that lost the names on its way is read by the data.
        let named = !event.name.is_empty();
        if named && !is_read(&event.name) {
            continue;
        }
        let data: Value = match serde_json::from_str(&event.data) {
            Ok(data) => data,
            // Skipped, since a path may add events of its own; but it may
            // as well be a part of the answer that did not arrive whole,
            // so an error after it does not open the stream.
            Err(_) if !named => {
                answer.began = true;
                continue;
            }
            Err(error) => {
                return StreamEnd::Broken(format!("unreadable {} event: {error}", event.name));
            }
        };
        let kind = if named {
            event.name.as_str()
        } else {
            data["type"].as_str().unwrap_or_default()
        };
        match kind {
            "message_start" => {
                answer.began = true;
                let message = &data["message"];
                answer.model = Some(message["model"].as_str().unwrap_or_default().to_owned());
                read_usage(&message["usage"], &mut answer.usage);
                read_extra_usage(&message["usage"], &mut answer.extra);
            }
            // A text block may open with text already in it. A fallback
            // block marks where another model took the answer over: the
            // text carries on after it, written by that model.
            "content_block_start" => {
                // The stop reason comes after all content. More content
                // means the answer goes on: it is whole again only when
                // the message says so again.
                answer.began = true;
                *stopped = false;
                let block = &data["content_block"];
                if block["type"] == "text" {
                    answer
                        .text
                        .push_str(block["text"].as_str().unwrap_or_default());
                } else if block["type"] == "fallback"
                    && let Some(model) = block["to"]["model"].as_str()
                {
                    answer.model = Some(model.to_owned());
                }
                let place = place(&data, answer.content.len());
                if place < answer.content.len() {
                    answer.content[place] = block.clone();
                } else {
                    answer.content.resize(place, Value::Null);
                    answer.content.push(block.clone());
                }
            }
            "content_block_delta" => {
                answer.began = true;
                *stopped = false;
                let delta = &data["delta"];
                if delta["type"] == "text_delta" {
                    answer
                        .text
                        .push_str(delta["text"].as_str().unwrap_or_default());
                }
                let place = place(&data, answer.content.len().saturating_sub(1));
                if delta["type"] == "input_json_delta" {
                    let piece = delta["partial_json"].as_str().unwrap_or_default();
                    match inputs.iter_mut().find(|(at, _)| *at == place) {
                        Some((_, input)) => input.push_str(piece),
                        None => inputs.push((place, piece.to_owned())),
                    }
                } else if let Some(block) = answer.content.get_mut(place) {
                    add_to_block(block, delta);
                }
            }
            // A tool's input is whole when its block closes.
            "content_block_stop" => {
                let place = place(&data, answer.content.len().saturating_sub(1));
                if let Some(at) = inputs.iter().position(|(at, _)| *at == place) {
                    let (_, input) = inputs.swap_remove(at);
                    if let Some(block) = answer.content.get_mut(place)
                        && let Ok(input) = serde_json::from_str::<Value>(&input)
                    {
                        block["input"] = input;
                    }
                }
            }
            "message_delta" => {
                answer.began = true;
                let delta = &data["delta"];
                if let Some(reason) = delta["stop_reason"].as_str() {
                    answer.stop_reason = Some(reason.to_owned());
                    *stopped = true;
                }
                // The details of a refusal belong to the message; read
                // them wherever the event carries them.
                for details in [&delta["stop_details"], &data["stop_details"]] {
                    if !details.is_null() {
                        answer.stop_details = details.clone();
                    }
                }
                read_usage(&data["usage"], &mut answer.usage);
                read_extra_usage(&data["usage"], &mut answer.extra);
            }
            "message_stop" => return StreamEnd::Complete,
            "error" => {
                let error = &data["error"];
                return StreamEnd::Failed {
                    kind: error["type"].as_str().unwrap_or("error").to_owned(),
                    message: error["message"].as_str().unwrap_or_default().to_owned(),
                };
            }
            _ => {}
        }
    }
    StreamEnd::Broken("the stream ended before the answer did".to_owned())
}

/// The place of the block an event is about: its `index`, or `otherwise`
/// when the event does not say.
fn place(data: &Value, otherwise: usize) -> usize {
    data["index"]
        .as_u64()
        .and_then(|index| usize::try_from(index).ok())
        // A stream is cut far below this many blocks; a larger place is
        // not one.
        .filter(|index| *index <= 100_000)
        .unwrap_or(otherwise)
}

/// Adds what a delta carries to its block: more text, more thought, the
/// thought's signature, or a page the text cites.
fn add_to_block(block: &mut Value, delta: &Value) {
    let (field, piece) = match delta["type"].as_str() {
        Some("text_delta") => ("text", &delta["text"]),
        Some("thinking_delta") => ("thinking", &delta["thinking"]),
        Some("signature_delta") => ("signature", &delta["signature"]),
        Some("citations_delta") => {
            if !block.is_object() {
                return;
            }
            match block["citations"].as_array_mut() {
                Some(citations) => citations.push(delta["citation"].clone()),
                None => block["citations"] = Value::Array(vec![delta["citation"].clone()]),
            }
            return;
        }
        _ => return,
    };
    if !block.is_object() {
        return;
    }
    let mut whole = block[field].as_str().unwrap_or_default().to_owned();
    whole.push_str(piece.as_str().unwrap_or_default());
    block[field] = Value::String(whole);
}

/// The counts beside the tokens sent and written. Tokens through the
/// cache are counted like the others: cumulative, and summed over
/// `iterations` when the answer took more than one step (each search or
/// fetch starts another). The tools the provider ran are counted for the
/// whole answer.
pub(crate) fn read_extra_usage(usage: &Value, into: &mut ExtraUsage) {
    if let Some(written) = usage["cache_creation_input_tokens"].as_u64() {
        into.cache_write_tokens = written;
    }
    if let Some(read) = usage["cache_read_input_tokens"].as_u64() {
        into.cache_read_tokens = read;
    }
    if let Some(iterations) = usage["iterations"].as_array()
        && !iterations.is_empty()
    {
        let sum = |count: &str| {
            iterations
                .iter()
                .filter_map(|iteration| iteration[count].as_u64())
                .fold(0u64, u64::saturating_add)
        };
        into.cache_write_tokens = sum("cache_creation_input_tokens");
        into.cache_read_tokens = sum("cache_read_input_tokens");
    }
    let tools = &usage["server_tool_use"];
    if let Some(searches) = tools["web_search_requests"].as_u64() {
        into.searches = searches;
    }
    if let Some(fetches) = tools["web_fetch_requests"].as_u64() {
        into.fetches = fetches;
    }
}

/// Counts are cumulative, and an event may leave one out: a count that is
/// absent keeps what an earlier event said. When the answer took more than
/// one attempt (a declined request that a fallback model answered),
/// `iterations` lists what each attempt used and the counts beside it are
/// only the last attempt's, so the use is their sum. That sum is the most
/// the answer can cost, not always the bill: an attempt that declined
/// before it wrote anything is billed only for some reasons, and each
/// attempt is billed at its own model's rate, while the sum is recorded
/// under the model that answered.
pub(crate) fn read_usage(usage: &Value, into: &mut TokenUsage) {
    if let Some(input) = usage["input_tokens"].as_u64() {
        into.input_tokens = input;
    }
    if let Some(output) = usage["output_tokens"].as_u64() {
        into.output_tokens = output;
    }
    if let Some(iterations) = usage["iterations"].as_array()
        && !iterations.is_empty()
    {
        let sum = |count: &str| {
            iterations
                .iter()
                .filter_map(|iteration| iteration[count].as_u64())
                .fold(0u64, u64::saturating_add)
        };
        into.input_tokens = sum("input_tokens");
        into.output_tokens = sum("output_tokens");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: &str = "event: message_start\n\
        data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_01\",\"type\":\"message\",\
        \"role\":\"assistant\",\"model\":\"claude-opus-5-5\",\"content\":[],\"stop_reason\":null,\
        \"usage\":{\"input_tokens\":412,\"output_tokens\":1}}}\n\n";

    fn text_block(index: u32, parts: &[&str]) -> String {
        let mut events = format!(
            "event: content_block_start\ndata: {{\"type\":\"content_block_start\",\"index\":{index},\
             \"content_block\":{{\"type\":\"text\",\"text\":\"\"}}}}\n\n"
        );
        for part in parts {
            let delta = serde_json::json!({
                "type": "content_block_delta",
                "index": index,
                "delta": {"type": "text_delta", "text": part},
            });
            events.push_str(&format!("event: content_block_delta\ndata: {delta}\n\n"));
        }
        events.push_str(&format!(
            "event: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":{index}}}\n\n"
        ));
        events
    }

    fn stop(reason: &str, output_tokens: u64) -> String {
        format!(
            "event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\
             \"{reason}\",\"stop_sequence\":null}},\"usage\":{{\"output_tokens\":{output_tokens}}}}}\n\n\
             event: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n"
        )
    }

    const THINKING: &str = "event: content_block_start\n\
        data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\
        \"thinking\":\"\",\"signature\":\"\"}}\n\n\
        event: ping\ndata: {\"type\": \"ping\"}\n\n\
        event: content_block_delta\n\
        data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\
        \"thinking\":\"\"}}\n\n\
        event: content_block_delta\n\
        data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\
        \"signature\":\"EqQBCkYIBxgCKkA0\"}}\n\n\
        event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n";

    #[test]
    fn a_whole_answer_joins_its_text_and_reports_the_final_usage() {
        let stream = [
            START,
            THINKING,
            &text_block(1, &["{\"themes\":[\"Apollo", " 13\",", "\"Voskhod 2\"]}"]),
            &stop("end_turn", 893),
        ]
        .concat();
        let answer = read(stream.as_bytes());
        assert_eq!(answer.end, StreamEnd::Complete);
        assert_eq!(answer.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(answer.text, "{\"themes\":[\"Apollo 13\",\"Voskhod 2\"]}");
        assert_eq!(answer.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(
            answer.usage,
            TokenUsage {
                input_tokens: 412,
                output_tokens: 893,
            }
        );
    }

    #[test]
    fn the_content_is_rebuilt_block_by_block_as_a_whole_message_holds_it() {
        let call = "event: content_block_start\n\
            data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"server_tool_use\",\
            \"id\":\"srvtoolu_01\",\"name\":\"web_search\",\"input\":{}}}\n\n\
            event: content_block_delta\n\
            data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\
            \"partial_json\":\"{\\\"query\\\":\\\"desk \"}}\n\n\
            event: content_block_delta\n\
            data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\
            \"partial_json\":\"lamps\\\"}\"}}\n\n\
            event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":1}\n\n";
        // A tool's result arrives whole when its block starts.
        let result = "event: content_block_start\n\
            data: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\
            \"web_search_tool_result\",\"tool_use_id\":\"srvtoolu_01\",\"content\":[{\"type\":\
            \"web_search_result\",\"url\":\"https://example.com/lamps\",\"title\":\"Lamps\"}]}}\n\n\
            event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":2}\n\n";
        let cited = "event: content_block_start\n\
            data: {\"type\":\"content_block_start\",\"index\":3,\"content_block\":{\"type\":\"text\",\
            \"text\":\"\",\"citations\":[]}}\n\n\
            event: content_block_delta\n\
            data: {\"type\":\"content_block_delta\",\"index\":3,\"delta\":{\"type\":\"citations_delta\",\
            \"citation\":{\"type\":\"web_search_result_location\",\"url\":\"https://example.com/lamps\",\
            \"title\":\"Lamps\",\"cited_text\":\"lights a desk\"}}}\n\n\
            event: content_block_delta\n\
            data: {\"type\":\"content_block_delta\",\"index\":3,\"delta\":{\"type\":\"text_delta\",\
            \"text\":\"It lights \"}}\n\n\
            event: content_block_delta\n\
            data: {\"type\":\"content_block_delta\",\"index\":3,\"delta\":{\"type\":\"text_delta\",\
            \"text\":\"a desk.\"}}\n\n\
            event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":3}\n\n";
        let end = "event: message_delta\n\
            data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"pause_turn\"},\"usage\":{\
            \"input_tokens\":30,\"output_tokens\":95,\"cache_creation_input_tokens\":5200,\
            \"cache_read_input_tokens\":410,\"server_tool_use\":{\"web_search_requests\":1,\
            \"web_fetch_requests\":0}}}\n\n\
            event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";
        let stream = [START, THINKING, call, result, cited, end].concat();
        let answer = read(stream.as_bytes());
        assert_eq!(answer.end, StreamEnd::Complete);
        assert_eq!(answer.stop_reason.as_deref(), Some("pause_turn"));
        assert_eq!(
            answer.content,
            [
                serde_json::json!({
                    "type": "thinking", "thinking": "", "signature": "EqQBCkYIBxgCKkA0",
                }),
                serde_json::json!({
                    "type": "server_tool_use", "id": "srvtoolu_01", "name": "web_search",
                    "input": { "query": "desk lamps" },
                }),
                serde_json::json!({
                    "type": "web_search_tool_result", "tool_use_id": "srvtoolu_01",
                    "content": [{
                        "type": "web_search_result", "url": "https://example.com/lamps",
                        "title": "Lamps",
                    }],
                }),
                serde_json::json!({
                    "type": "text", "text": "It lights a desk.",
                    "citations": [{
                        "type": "web_search_result_location",
                        "url": "https://example.com/lamps", "title": "Lamps",
                        "cited_text": "lights a desk",
                    }],
                }),
            ]
        );
        assert_eq!(answer.text, "It lights a desk.");
        // Without a list of steps, the counts beside the tokens are the
        // answer's own.
        assert_eq!(
            answer.extra,
            ExtraUsage {
                cache_write_tokens: 5_200,
                cache_read_tokens: 410,
                searches: 1,
                fetches: 0,
            }
        );
        assert_eq!(answer.usage.input_tokens, 30);
    }

    #[test]
    fn tokens_through_the_cache_are_summed_over_the_steps_of_an_answer() {
        let stream = [
            START,
            &text_block(0, &["Notes."]),
            "event: message_delta\n\
             data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\
             \"usage\":{\"input_tokens\":12,\"output_tokens\":40,\"cache_creation_input_tokens\":900,\
             \"cache_read_input_tokens\":7000,\"iterations\":[\
             {\"type\":\"message\",\"input_tokens\":412,\"output_tokens\":80,\
             \"cache_creation_input_tokens\":6100,\"cache_read_input_tokens\":0},\
             {\"type\":\"message\",\"input_tokens\":12,\"output_tokens\":40,\
             \"cache_creation_input_tokens\":900,\"cache_read_input_tokens\":7000}],\
             \"server_tool_use\":{\"web_search_requests\":0,\"web_fetch_requests\":1}}}\n\n\
             event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        ]
        .concat();
        let answer = read(stream.as_bytes());
        assert_eq!(
            answer.extra,
            ExtraUsage {
                cache_write_tokens: 7_000,
                cache_read_tokens: 7_000,
                searches: 0,
                fetches: 1,
            }
        );
        assert_eq!(
            answer.usage,
            TokenUsage {
                input_tokens: 424,
                output_tokens: 120,
            }
        );
        // An answer that used neither says so.
        let plain = [START, &text_block(0, &["ok"]), &stop("end_turn", 2)].concat();
        assert_eq!(read(plain.as_bytes()).extra, ExtraUsage::default());
    }

    #[test]
    fn text_blocks_on_both_sides_of_a_fallback_are_one_answer() {
        // A request declined midway continues on the fallback model: the
        // boundary is a block of its own and the text carries on after it.
        let fallback = "event: content_block_start\n\
            data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"fallback\",\
            \"from\":{\"model\":\"claude-opus-5-5\"},\"to\":{\"model\":\"claude-opus-4-8\"},\
            \"trigger\":{\"type\":\"refusal\",\"category\":\"cyber\"}}}\n\n\
            event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":1}\n\n";
        let stream = [
            START,
            &text_block(0, &["Once upon"]),
            fallback,
            &text_block(2, &[" a time."]),
            &stop("end_turn", 40),
        ]
        .concat();
        let answer = read(stream.as_bytes());
        assert_eq!(answer.end, StreamEnd::Complete);
        assert_eq!(answer.text, "Once upon a time.");
        assert_eq!(
            answer.model.as_deref(),
            Some("claude-opus-4-8"),
            "the model that finished the answer"
        );
    }

    #[test]
    fn a_refusal_keeps_its_details() {
        let stream = [
            START,
            "event: message_delta\n\
             data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"refusal\",\"stop_details\":\
             {\"type\":\"refusal\",\"category\":\"cyber\",\"explanation\":\"Not something I can help with.\"}},\
             \"usage\":{\"output_tokens\":3}}\n\n\
             event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        ]
        .concat();
        let answer = read(stream.as_bytes());
        assert_eq!(answer.end, StreamEnd::Complete);
        assert_eq!(answer.stop_reason.as_deref(), Some("refusal"));
        assert_eq!(answer.stop_details["category"], "cyber");
        assert!(answer.text.is_empty());
    }

    #[test]
    fn an_answer_cut_at_the_token_limit_is_whole_and_says_so() {
        let stream = [
            START,
            &text_block(0, &["{\"scenes\":[{\"pro"]),
            &stop("max_tokens", 16_000),
        ]
        .concat();
        let answer = read(stream.as_bytes());
        assert_eq!(answer.end, StreamEnd::Complete);
        assert_eq!(answer.stop_reason.as_deref(), Some("max_tokens"));
        assert_eq!(answer.usage.output_tokens, 16_000);
    }

    #[test]
    fn a_stream_that_stops_midway_keeps_what_arrived() {
        let whole = [START, &text_block(0, &["first part, ", "second part"])].concat();
        // Cut inside the last delta's event.
        let cut = &whole[..whole.find("second part").unwrap()];
        let answer = read(cut.as_bytes());
        assert!(matches!(answer.end, StreamEnd::Broken(_)));
        assert!(answer.started());
        assert_eq!(answer.text, "first part, ");
        assert_eq!(answer.usage.input_tokens, 412);
        assert_eq!(answer.stop_reason, None);
    }

    #[test]
    fn a_read_error_breaks_the_stream_with_its_reason() {
        struct Silent<'a>(&'a [u8]);
        impl Read for Silent<'_> {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                if self.0.is_empty() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "no data for 600 seconds",
                    ));
                }
                let count = buffer.len().min(self.0.len());
                buffer[..count].copy_from_slice(&self.0[..count]);
                self.0 = &self.0[count..];
                Ok(count)
            }
        }
        let stream = [START, &text_block(0, &["so far"])].concat();
        let answer = read(Silent(stream.as_bytes()));
        assert_eq!(
            answer.end,
            StreamEnd::Broken("no data for 600 seconds".to_owned())
        );
        assert_eq!(answer.text, "so far");
    }

    #[test]
    fn an_error_event_ends_the_stream_with_the_apis_reason() {
        let error = "event: error\n\
            data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n";
        let before = read(error.as_bytes());
        assert_eq!(
            before.end,
            StreamEnd::Failed {
                kind: "overloaded_error".to_owned(),
                message: "Overloaded".to_owned(),
            }
        );
        assert!(!before.started(), "nothing was generated");

        let midway = [START, &text_block(0, &["half an ans"]), error].concat();
        let after = read(midway.as_bytes());
        assert!(matches!(after.end, StreamEnd::Failed { .. }));
        assert!(after.started());
        assert_eq!(after.text, "half an ans");
    }

    #[test]
    fn an_unreadable_event_breaks_the_stream() {
        let stream = [START, "event: content_block_delta\ndata: {\"type\":\n\n"].concat();
        let answer = read(stream.as_bytes());
        assert!(
            matches!(&answer.end, StreamEnd::Broken(detail) if detail.contains("content_block_delta"))
        );
    }

    #[test]
    fn events_it_does_not_know_are_skipped() {
        let stream = [
            START,
            "event: something_new\ndata: not even json\n\n",
            &text_block(0, &["ok"]),
            &stop("end_turn", 2),
        ]
        .concat();
        let answer = read(stream.as_bytes());
        assert_eq!(answer.end, StreamEnd::Complete);
        assert_eq!(answer.text, "ok");
    }

    #[test]
    fn an_empty_body_is_a_stream_that_never_started() {
        let answer = read(&b""[..]);
        assert!(matches!(answer.end, StreamEnd::Broken(_)));
        assert!(!answer.started());
    }

    #[test]
    fn an_answer_that_gave_its_stop_reason_is_whole_without_the_last_event() {
        // The connection closed between the stop reason and `message_stop`.
        let stopped = stop("end_turn", 12);
        let cut = stopped.split("event: message_stop").next().unwrap();
        let stream = [START, &text_block(0, &["All of it."]), cut].concat();
        let answer = read(stream.as_bytes());
        assert_eq!(answer.end, StreamEnd::Complete);
        assert_eq!(answer.text, "All of it.");
        assert_eq!(answer.usage.output_tokens, 12);

        // An error after the stop reason does not take the answer back.
        let stream = [
            START,
            &text_block(0, &["All of it."]),
            cut,
            "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"api_error\",\
             \"message\":\"Internal server error\"}}\n\n",
        ]
        .concat();
        assert_eq!(read(stream.as_bytes()).end, StreamEnd::Complete);
    }

    #[test]
    fn a_stop_reason_followed_by_more_of_the_answer_is_not_its_end() {
        // Not a flow the API documents: if more content follows a stop
        // reason, the answer was not over, and a stream that then drops
        // did not bring it whole.
        let stream = [
            START,
            &text_block(0, &["half"]),
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":\
             {\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":40}}\n\n",
            &text_block(1, &[" more"]),
        ]
        .concat();
        let answer = read(stream.as_bytes());
        assert_eq!(answer.text, "half more");
        assert!(matches!(answer.end, StreamEnd::Broken(_)), "{answer:?}");

        // When the message then ends without another reason, the one it
        // gave stands: a refusal is not forgotten.
        let stream = [
            START,
            &text_block(0, &["half"]),
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":\
             {\"stop_reason\":\"refusal\"},\"usage\":{\"output_tokens\":40}}\n\n",
            &text_block(1, &[" more"]),
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        ]
        .concat();
        let answer = read(stream.as_bytes());
        assert_eq!(answer.end, StreamEnd::Complete);
        assert_eq!(answer.stop_reason.as_deref(), Some("refusal"));
    }

    #[test]
    fn an_answer_whose_start_could_not_be_read_still_started() {
        // Text arrived although no `message_start` was read: an error
        // after it is not one that opens the stream.
        let stream = [
            &text_block(0, &["half"]),
            "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\
             \"message\":\"Overloaded\"}}\n\n",
        ]
        .concat();
        let answer = read(stream.as_bytes());
        assert_eq!(answer.model, None);
        assert!(answer.started());
        assert!(!read(&b""[..]).started());

        // Thought alone, or an event that cannot be read, is of an answer
        // too.
        let overloaded = "event: error\ndata: {\"type\":\"error\",\"error\":\
            {\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n";
        assert!(read([THINKING, overloaded].concat().as_bytes()).started());
        let garbled = "data: {\"type\":\"message_start\",\"message\":{\"id\n\n";
        assert!(read([garbled, overloaded].concat().as_bytes()).started());
        // An error alone, or after a keep-alive, opens the stream.
        let ping = "event: ping\ndata: {\"type\": \"ping\"}\n\n";
        assert!(!read([ping, overloaded].concat().as_bytes()).started());
    }

    #[test]
    fn events_without_a_name_are_read_by_their_type() {
        let stream = [
            START,
            &text_block(0, &["Named or not."]),
            &stop("end_turn", 9),
        ]
        .concat();
        let unnamed: String = stream
            .lines()
            .filter(|line| !line.starts_with("event:"))
            .map(|line| format!("{line}\n"))
            .collect();
        let answer = read(unnamed.as_bytes());
        assert_eq!(answer.end, StreamEnd::Complete);
        assert_eq!(answer.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(answer.text, "Named or not.");
        assert_eq!(answer.usage.input_tokens, 412);
    }

    #[test]
    fn an_answer_that_took_two_attempts_is_billed_for_both() {
        // The counts beside `iterations` are the last attempt's alone.
        let stream = [
            START,
            &text_block(0, &["Once upon a time."]),
            "event: message_delta\n\
             data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\
             \"usage\":{\"input_tokens\":480,\"output_tokens\":40,\"iterations\":[\
             {\"type\":\"message\",\"model\":\"claude-opus-5-5\",\"input_tokens\":412,\
             \"output_tokens\":3000},\
             {\"type\":\"fallback_message\",\"model\":\"claude-opus-4-8\",\"input_tokens\":480,\
             \"output_tokens\":40}]}}\n\n\
             event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        ]
        .concat();
        let answer = read(stream.as_bytes());
        assert_eq!(
            answer.usage,
            TokenUsage {
                input_tokens: 892,
                output_tokens: 3_040,
            }
        );
    }
}
