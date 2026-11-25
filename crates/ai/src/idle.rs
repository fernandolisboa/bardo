//! A limit on silence for answers read as they arrive, and a way to give
//! one up. A streamed answer may rightly take many minutes, so no clock can
//! bound the whole of it; what tells a dead connection from a long answer
//! is that nothing arrives at all (a provider that streams sends
//! keep-alives while it works). And an answer nobody wants any more (its
//! job was cancelled, the app is closing) is given up as soon as its
//! reader is told, instead of being read, and billed, to its end.

use std::io::Read;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, sync_channel};
use std::time::{Duration, Instant};

use bardo_domain::Stop;

use crate::http::TransportError;
use crate::http::{BinaryResponse, HttpRequest, HttpResponse, StreamedResponse, Transport};

/// How often a wait for an answer asks whether to give it up.
pub const STOP_POLL: Duration = Duration::from_millis(100);

/// Reads `inner` and fails with `TimedOut` when no byte arrives for
/// `idle`, or once its `Stop` asks. The body is read on a thread of its
/// own, which ends with the body or, once this reader is dropped, at its
/// next read, and closes the connection there: after a silence or a stop
/// that thread and its connection stay until the next byte arrives or the
/// transport's own bound on the body ends them, since nothing here can
/// close a connection that says nothing.
pub struct IdleTimeout {
    chunks: Receiver<std::io::Result<Vec<u8>>>,
    idle: Duration,
    stop: Stop,
    pending: Vec<u8>,
    at: usize,
    ended: bool,
    timed_out: bool,
    given_up: bool,
}

impl IdleTimeout {
    const CHUNK: usize = 8 * 1024;

    pub fn new(inner: Box<dyn Read + Send>, idle: Duration) -> Self {
        Self::until(inner, idle, Stop::never())
    }

    /// `new`, given up once `stop` asks.
    pub fn until(mut inner: Box<dyn Read + Send>, idle: Duration, stop: Stop) -> Self {
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
                // A reader that is gone no longer wants the body: dropping
                // it here closes its connection.
                if sender.send(read).is_err() || failed {
                    break;
                }
            }
        });
        Self {
            chunks,
            idle,
            stop,
            pending: Vec::new(),
            at: 0,
            ended: false,
            timed_out: false,
            given_up: false,
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

    fn gave_up() -> std::io::Error {
        std::io::Error::other("the answer was given up")
    }
}

impl Read for IdleTimeout {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        // An answer given up stays given up, with whatever of it waits
        // here unread.
        if self.given_up || self.stop.asked() {
            self.given_up = true;
            return Err(Self::gave_up());
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
            let silent_at = Instant::now() + self.idle;
            loop {
                let left = silent_at.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    self.timed_out = true;
                    return Err(self.silence());
                }
                // Waits in short steps only when somebody may stop it.
                let step = if self.stop.may_ask() {
                    left.min(STOP_POLL)
                } else {
                    left
                };
                match self.chunks.recv_timeout(step) {
                    Ok(Ok(chunk)) => {
                        self.pending = chunk;
                        self.at = 0;
                        break;
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
                        if self.stop.asked() {
                            self.given_up = true;
                            return Err(Self::gave_up());
                        }
                    }
                }
            }
        }
        let count = buffer.len().min(self.pending.len() - self.at);
        buffer[..count].copy_from_slice(&self.pending[self.at..self.at + count]);
        self.at += count;
        Ok(count)
    }
}

/// A transport whose streamed answers give up after `idle` of silence, and
/// as soon as the caller's `Stop` asks. Everything else goes to the
/// transport as it is.
pub struct Watched<T> {
    transport: Arc<T>,
    idle: Duration,
}

impl<T> Watched<T> {
    pub fn new(transport: T, idle: Duration) -> Self {
        Self {
            transport: Arc::new(transport),
            idle,
        }
    }
}

impl<T: Transport + 'static> Watched<T> {
    /// Sends `request` on a thread of its own and waits for the answer to
    /// begin, giving the wait up once `stop` asks. The request cannot be
    /// taken back: the thread ends when the transport answers or gives up
    /// by its own bounds, and drops what it got, which closes the
    /// connection.
    fn begin(
        &self,
        request: &HttpRequest,
        stop: &Stop,
    ) -> Result<StreamedResponse, TransportError> {
        if stop.asked() {
            return Err(TransportError::not_sent(
                "given up before the request was sent",
            ));
        }
        let (sender, begun) = sync_channel(1);
        let transport = Arc::clone(&self.transport);
        let request = request.clone();
        std::thread::spawn(move || {
            let _ = sender.send(transport.send_streamed(&request));
        });
        loop {
            match begun.recv_timeout(STOP_POLL) {
                Ok(response) => return response,
                Err(RecvTimeoutError::Timeout) if stop.asked() => {
                    return Err(TransportError::unanswered(
                        "given up before the answer began",
                    ));
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(TransportError::unanswered(
                        "the request ended without an answer",
                    ));
                }
            }
        }
    }
}

impl<T: Transport + 'static> Transport for Watched<T> {
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
        self.send_streamed_until(request, &Stop::never())
    }

    fn send_streamed_until(
        &self,
        request: &HttpRequest,
        stop: &Stop,
    ) -> Result<StreamedResponse, TransportError> {
        let response = if stop.may_ask() {
            self.begin(request, stop)?
        } else {
            self.transport.send_streamed(request)?
        };
        Ok(StreamedResponse {
            status: response.status,
            headers: response.headers,
            body: Box::new(IdleTimeout::until(response.body, self.idle, stop.clone())),
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

    /// A stop that asks once `flag` is set.
    fn stop_on(flag: &Arc<std::sync::atomic::AtomicBool>) -> Stop {
        let flag = Arc::clone(flag);
        Stop::when(move || flag.load(std::sync::atomic::Ordering::SeqCst))
    }

    /// Sets `flag` after `after`, from another thread.
    fn set_later(flag: &Arc<std::sync::atomic::AtomicBool>, after: Duration) {
        let flag = Arc::clone(flag);
        std::thread::spawn(move || {
            std::thread::sleep(after);
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        });
    }

    #[test]
    fn a_silent_body_is_given_up_as_soon_as_its_stop_asks() {
        let asked = Arc::default();
        let (feed, body) = channel();
        // The limit on silence is far away: only the stop ends this read.
        let mut reader = IdleTimeout::until(
            Box::new(Fed(body)),
            Duration::from_secs(60),
            stop_on(&asked),
        );
        feed.send(Ok(b"start".to_vec())).unwrap();
        let mut buffer = [0u8; 16];
        assert_eq!(reader.read(&mut buffer).unwrap(), 5);
        set_later(&asked, Duration::from_millis(200));
        let started = Instant::now();
        let error = reader.read(&mut buffer).unwrap_err();
        assert!(error.to_string().contains("given up"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(5));
        // What arrives after that is not read: the answer stays given up.
        feed.send(Ok(b"more".to_vec())).unwrap();
        let error = reader.read(&mut buffer).unwrap_err();
        assert!(error.to_string().contains("given up"), "{error}");
    }

    #[test]
    fn a_stop_that_never_asks_changes_nothing() {
        let asked = Arc::default();
        let (feed, body) = channel();
        let mut reader = IdleTimeout::until(
            Box::new(Fed(body)),
            Duration::from_millis(500),
            stop_on(&asked),
        );
        feed.send(Ok(b"hello".to_vec())).unwrap();
        let mut buffer = [0u8; 16];
        assert_eq!(reader.read(&mut buffer).unwrap(), 5);
        // Silence is still what ends a body nobody stopped.
        let error = reader.read(&mut buffer).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        drop(feed);
    }

    #[test]
    fn giving_an_answer_up_closes_a_connection_that_would_go_on() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (closed, seen_closed) = channel::<()>();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request).unwrap();
            // An answer far longer than anyone reads here, a keep-alive at
            // a time, for as long as the other side takes it.
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100000000\r\n\r\n")
                .unwrap();
            while stream
                .write_all(b"event: ping\n\n")
                .and_then(|()| stream.flush())
                .is_ok()
            {
                std::thread::sleep(Duration::from_millis(20));
            }
            let _ = closed.send(());
        });
        let asked = Arc::default();
        let stop = stop_on(&asked);
        let transport = Watched::new(
            UreqTransport::without_proxy(Duration::from_secs(60)),
            Duration::from_secs(60),
        );
        let mut response = transport
            .send_streamed_until(&HttpRequest::get(format!("http://{address}/stream")), &stop)
            .unwrap();
        let mut buffer = [0u8; 13];
        response.body.read_exact(&mut buffer).unwrap();
        assert_eq!(&buffer, b"event: ping\n\n");
        // The job is cancelled: its reader fails at its next read and is
        // dropped, as the adapter does.
        asked.store(true, std::sync::atomic::Ordering::SeqCst);
        let mut rest = Vec::new();
        let error = response.body.read_to_end(&mut rest).unwrap_err();
        assert!(error.to_string().contains("given up"), "{error}");
        drop(response);
        // The server finds the connection closed within seconds, although
        // it had far more to send.
        seen_closed
            .recv_timeout(Duration::from_secs(10))
            .expect("the connection was closed");
        server.join().unwrap();
    }

    #[test]
    fn a_wait_for_an_answer_to_begin_is_given_up() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (release, held) = channel::<()>();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request).unwrap();
            // Takes the request and says nothing.
            let _ = held.recv();
        });
        let asked = Arc::default();
        let stop = stop_on(&asked);
        let transport = Watched::new(
            UreqTransport::without_proxy(Duration::from_secs(60)),
            Duration::from_secs(60),
        );
        set_later(&asked, Duration::from_millis(300));
        let started = Instant::now();
        let Err(error) = transport
            .send_streamed_until(&HttpRequest::get(format!("http://{address}/stream")), &stop)
        else {
            panic!("given up");
        };
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(error.0.contains("given up"), "{}", error.0);
        assert!(
            !error.never_left(),
            "the request left: the server may have acted on it"
        );
        // Asked before anything is sent, nothing leaves.
        let Err(error) = transport
            .send_streamed_until(&HttpRequest::get(format!("http://{address}/stream")), &stop)
        else {
            panic!("given up");
        };
        assert!(error.never_left());
        release.send(()).unwrap();
        server.join().unwrap();
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
