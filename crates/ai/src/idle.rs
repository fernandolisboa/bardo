//! A limit on silence for answers read as they arrive. A streamed answer
//! may rightly take many minutes, so no clock can bound the whole of it;
//! what tells a dead connection from a long answer is that nothing arrives
//! at all (a provider that streams sends keep-alives while it works).

use std::io::Read;
use std::sync::mpsc::{Receiver, RecvTimeoutError, sync_channel};
use std::time::Duration;

use crate::http::TransportError;
use crate::http::{BinaryResponse, HttpRequest, HttpResponse, StreamedResponse, Transport};

/// Reads `inner` and fails with `TimedOut` when no byte arrives for
/// `idle`. The body is read on a thread of its own, which ends with the
/// body or, once this reader is dropped, at its next read: after a silence
/// that thread and its connection stay until the transport's own bound on
/// the body ends them, since nothing here can close the connection.
pub struct IdleTimeout {
    chunks: Receiver<std::io::Result<Vec<u8>>>,
    idle: Duration,
    pending: Vec<u8>,
    at: usize,
    ended: bool,
    timed_out: bool,
}

impl IdleTimeout {
    const CHUNK: usize = 8 * 1024;

    pub fn new(mut inner: Box<dyn Read + Send>, idle: Duration) -> Self {
        // A small queue: the reading thread waits for the consumer instead
        // of holding an unbounded answer in memory.
        let (sender, chunks) = sync_channel(4);
        std::thread::spawn(move || {
            let mut buffer = vec![0; Self::CHUNK];
            loop {
                let read = match inner.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => Ok(buffer[..count].to_vec()),
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => Err(error),
                };
                let failed = read.is_err();
                if sender.send(read).is_err() || failed {
                    break;
                }
            }
        });
        Self {
            chunks,
            idle,
            pending: Vec::new(),
            at: 0,
            ended: false,
            timed_out: false,
        }
    }
}

impl IdleTimeout {
    fn silence(&self) -> std::io::Error {
        std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("no data for {} seconds", self.idle.as_secs()),
        )
    }
}

impl Read for IdleTimeout {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.at == self.pending.len() {
            // Silence stays an error however often it is asked about, so
            // a reader in between cannot take it for the end of the body.
            if self.timed_out {
                return Err(self.silence());
            }
            if self.ended {
                return Ok(0);
            }
            match self.chunks.recv_timeout(self.idle) {
                Ok(Ok(chunk)) => {
                    self.pending = chunk;
                    self.at = 0;
                }
                Ok(Err(error)) => {
                    self.ended = true;
                    return Err(error);
                }
                // The thread is done: the body ended.
                Err(RecvTimeoutError::Disconnected) => {
                    self.ended = true;
                    return Ok(0);
                }
                Err(RecvTimeoutError::Timeout) => {
                    self.timed_out = true;
                    return Err(self.silence());
                }
            }
        }
        let count = buffer.len().min(self.pending.len() - self.at);
        buffer[..count].copy_from_slice(&self.pending[self.at..self.at + count]);
        self.at += count;
        Ok(count)
    }
}

/// A transport whose streamed answers give up after `idle` of silence.
/// Everything else goes to the transport as it is.
pub struct Watched<T> {
    transport: T,
    idle: Duration,
}

impl<T> Watched<T> {
    pub fn new(transport: T, idle: Duration) -> Self {
        Self { transport, idle }
    }
}

impl<T: Transport> Transport for Watched<T> {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError> {
        self.transport.send(request)
    }

    fn send_for_bytes(
        &self,
        request: &HttpRequest,
        max_bytes: u64,
    ) -> Result<BinaryResponse, TransportError> {
        self.transport.send_for_bytes(request, max_bytes)
    }

    fn send_streamed(&self, request: &HttpRequest) -> Result<StreamedResponse, TransportError> {
        let response = self.transport.send_streamed(request)?;
        Ok(StreamedResponse {
            status: response.status,
            headers: response.headers,
            body: Box::new(IdleTimeout::new(response.body, self.idle)),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::sync::mpsc::{Sender, channel};

    use super::*;
    use crate::http::UreqTransport;

    /// A body the test feeds: each message is a read, a closed channel the
    /// end.
    struct Fed(Receiver<std::io::Result<Vec<u8>>>);

    impl Read for Fed {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            match self.0.recv() {
                Ok(Ok(bytes)) => {
                    buffer[..bytes.len()].copy_from_slice(&bytes);
                    Ok(bytes.len())
                }
                Ok(Err(error)) => Err(error),
                Err(_) => Ok(0),
            }
        }
    }

    fn fed(idle: Duration) -> (Sender<std::io::Result<Vec<u8>>>, IdleTimeout) {
        let (feed, body) = channel();
        (feed, IdleTimeout::new(Box::new(Fed(body)), idle))
    }

    #[test]
    fn a_body_is_read_whole_however_it_is_cut() {
        let (feed, mut reader) = fed(Duration::from_secs(5));
        feed.send(Ok(b"hello ".to_vec())).unwrap();
        feed.send(Ok(b"world".to_vec())).unwrap();
        drop(feed);
        // A buffer smaller than a chunk: nothing is lost between reads.
        let mut text = Vec::new();
        let mut small = [0u8; 4];
        loop {
            let count = reader.read(&mut small).unwrap();
            if count == 0 {
                break;
            }
            text.extend_from_slice(&small[..count]);
        }
        assert_eq!(text, b"hello world");
        assert_eq!(reader.read(&mut small).unwrap(), 0, "the end stays the end");
    }

    #[test]
    fn silence_past_the_limit_is_a_timeout() {
        let (feed, mut reader) = fed(Duration::from_millis(500));
        feed.send(Ok(b"start".to_vec())).unwrap();
        let mut buffer = [0u8; 16];
        assert_eq!(reader.read(&mut buffer).unwrap(), 5);
        // The feed stays open and silent.
        let error = reader.read(&mut buffer).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(error.to_string().contains("no data"));
        // Asked again, it is still silence, not the end of the body.
        let error = reader.read(&mut buffer).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        drop(feed);
    }

    #[test]
    fn data_that_keeps_coming_is_never_a_timeout() {
        // Longer in all than the limit on silence, and no gap as long.
        let (feed, mut reader) = fed(Duration::from_millis(1_500));
        let writer = std::thread::spawn(move || {
            for _ in 0..8 {
                std::thread::sleep(Duration::from_millis(250));
                feed.send(Ok(b"tick".to_vec())).unwrap();
            }
        });
        let mut text = String::new();
        reader.read_to_string(&mut text).unwrap();
        assert_eq!(text, "tick".repeat(8));
        writer.join().unwrap();
    }

    #[test]
    fn a_read_error_comes_through() {
        let (feed, mut reader) = fed(Duration::from_secs(5));
        feed.send(Err(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "reset by peer",
        )))
        .unwrap();
        let error = reader.read(&mut [0u8; 8]).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);
    }

    #[test]
    fn a_server_that_goes_silent_mid_answer_times_out_on_the_wire() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (release, held) = channel::<()>();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request).unwrap();
            // Says more is coming, sends a part, then nothing.
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nevent: ping\n\n")
                .unwrap();
            stream.flush().unwrap();
            let _ = held.recv();
        });
        let transport = Watched::new(
            UreqTransport::without_proxy(Duration::from_secs(20)),
            Duration::from_millis(500),
        );
        let mut response = transport
            .send_streamed(&HttpRequest::get(format!("http://{address}/stream")))
            .unwrap();
        assert_eq!(response.status, 200);
        let started = std::time::Instant::now();
        let mut text = String::new();
        let error = response.body.read_to_string(&mut text).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert_eq!(text, "event: ping\n\n", "what arrived is kept");
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "long before the transport's own timeout"
        );
        release.send(()).unwrap();
        server.join().unwrap();
    }
}
