//! Server-sent events: how a provider sends an answer while it is still
//! writing it. An event is a run of `field: value` lines ended by a blank
//! line; Bardo reads the `event` and `data` fields and skips the rest
//! (comments, `id`, `retry`).

use std::io::{BufRead, BufReader, Read};

/// One event of a stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// The `event` field; empty when the event has none.
    pub name: String,
    /// The `data` lines, joined by line breaks.
    pub data: String,
}

/// The events of a stream, in order, read as they arrive. A read error
/// ends the stream with that error; an event the stream ends in the middle
/// of is dropped, as it never finished.
pub struct Events<R> {
    lines: std::io::Lines<BufReader<R>>,
    begun: bool,
    failed: bool,
}

impl<R: Read> Events<R> {
    pub fn new(body: R) -> Self {
        Self {
            lines: BufReader::new(body).lines(),
            begun: false,
            failed: false,
        }
    }
}

impl<R: Read> Iterator for Events<R> {
    type Item = std::io::Result<Event>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        let mut name = String::new();
        let mut data: Vec<String> = Vec::new();
        let mut any = false;
        loop {
            let line = match self.lines.next()? {
                // A byte-order mark may open the stream.
                Ok(line) if !std::mem::replace(&mut self.begun, true) => line
                    .strip_prefix('\u{feff}')
                    .map_or(line.clone(), str::to_owned),
                Ok(line) => line,
                Err(error) => {
                    self.failed = true;
                    return Some(Err(error));
                }
            };
            if line.is_empty() {
                // A blank line ends the event; blank lines between events
                // end nothing.
                if any {
                    return Some(Ok(Event {
                        name,
                        data: data.join("\n"),
                    }));
                }
                continue;
            }
            // A line starting with a colon is a comment (a keep-alive).
            if line.starts_with(':') {
                continue;
            }
            let (field, value) = line.split_once(':').unwrap_or((line.as_str(), ""));
            let value = value.strip_prefix(' ').unwrap_or(value);
            match field {
                "event" => {
                    name = value.to_owned();
                    any = true;
                }
                "data" => {
                    data.push(value.to_owned());
                    any = true;
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(stream: &str) -> Vec<Event> {
        Events::new(stream.as_bytes())
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn event(name: &str, data: &str) -> Event {
        Event {
            name: name.to_owned(),
            data: data.to_owned(),
        }
    }

    #[test]
    fn events_end_at_a_blank_line() {
        let stream = "event: message_start\ndata: {\"a\":1}\n\nevent: ping\ndata: {}\n\n";
        assert_eq!(
            events(stream),
            [event("message_start", "{\"a\":1}"), event("ping", "{}")]
        );
    }

    #[test]
    fn windows_line_endings_read_the_same() {
        let stream = "event: ping\r\ndata: {}\r\n\r\nevent: done\r\ndata: 1\r\n\r\n";
        assert_eq!(events(stream), [event("ping", "{}"), event("done", "1")]);
    }

    #[test]
    fn data_lines_of_one_event_are_joined() {
        assert_eq!(
            events("data: first\ndata: second\n\n"),
            [event("", "first\nsecond")]
        );
    }

    #[test]
    fn comments_unknown_fields_and_extra_blank_lines_are_skipped() {
        let stream = ": keep-alive\n\n\nid: 7\nretry: 1000\nevent: ping\ndata:{}\n\n\n";
        assert_eq!(events(stream), [event("ping", "{}")]);
    }

    #[test]
    fn a_byte_order_mark_at_the_start_is_not_part_of_the_first_field() {
        assert_eq!(
            events("\u{feff}event: ping\ndata: {}\n\n"),
            [event("ping", "{}")]
        );
    }

    #[test]
    fn only_one_space_after_the_colon_is_dropped() {
        assert_eq!(events("data:  two spaces\n\n"), [event("", " two spaces")]);
    }

    #[test]
    fn an_event_the_stream_ends_inside_is_dropped() {
        assert_eq!(
            events("event: a\ndata: 1\n\nevent: b\ndata: {\"cut"),
            [event("a", "1")]
        );
    }

    #[test]
    fn a_read_error_ends_the_stream_with_it() {
        struct Breaks(bool);
        impl Read for Breaks {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                if self.0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "no data for 60 seconds",
                    ));
                }
                self.0 = true;
                let bytes = b"event: a\ndata: 1\n\nevent: b\n";
                buffer[..bytes.len()].copy_from_slice(bytes);
                Ok(bytes.len())
            }
        }
        let mut events = Events::new(Breaks(false));
        assert_eq!(events.next().unwrap().unwrap(), event("a", "1"));
        let error = events.next().unwrap().unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(events.next().is_none());
    }
}
