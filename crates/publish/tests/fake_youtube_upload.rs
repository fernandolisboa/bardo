//! A whole upload against a fake YouTube on `127.0.0.1` over real HTTP:
//! the 308 "resume incomplete" answers must reach the adapter as they are,
//! not be taken for redirects, and the file must arrive whole. No call
//! leaves the machine.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use bardo_ai::http::UreqTransport;
use bardo_domain::{
    SecretText, UploadError, UploadOutcome, UploadRun, UploadedVideo, VideoUpload, VideoUploader,
    Visibility,
};
use bardo_publish::{GoogleEndpoints, YouTubeUploader};

const SIZE: usize = 600_000;

/// What the fake YouTube received.
#[derive(Default)]
struct YouTube {
    file: Mutex<Vec<u8>>,
    /// Drops the first chunk on the floor, as a broken connection would.
    fail_first_chunk: Mutex<bool>,
}

fn serve(youtube: Arc<YouTube>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let base = address.clone();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            handle(stream, &youtube, &base);
        }
    });
    address
}

fn handle(mut stream: TcpStream, youtube: &YouTube, base: &str) {
    let mut raw = Vec::new();
    let mut chunk = [0; 65536];
    let (head, mut body) = loop {
        let read = stream.read(&mut chunk).unwrap();
        if read == 0 {
            return;
        }
        raw.extend_from_slice(&chunk[..read]);
        if let Some(at) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break (
                String::from_utf8(raw[..at].to_vec()).unwrap(),
                raw[at + 4..].to_vec(),
            );
        }
    };
    let header = |name: &str| {
        head.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name)
                .then(|| value.trim().to_owned())
        })
    };
    let length: usize = header("content-length")
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    while body.len() < length {
        let read = stream.read(&mut chunk).unwrap();
        body.extend_from_slice(&chunk[..read]);
    }
    let request_line = head.lines().next().unwrap_or_default().to_owned();
    let answer = if request_line.starts_with("POST /upload/youtube/v3/videos") {
        format!(
            "HTTP/1.1 200 OK\r\nLocation: {base}/upload/youtube/v3/videos?upload_id=fake\r\nContent-Length: 0\r\n\r\n"
        )
    } else if request_line.starts_with("PUT /upload/youtube/v3/videos?upload_id=fake") {
        let range = header("content-range").unwrap_or_default();
        let mut file = youtube.file.lock().unwrap();
        if range.starts_with("bytes */") {
            // A status query.
        } else {
            let mut fail = youtube.fail_first_chunk.lock().unwrap();
            if *fail {
                *fail = false;
                drop(file);
                // Close without answering: the client sees a broken request.
                return;
            }
            let first: usize = range
                .trim_start_matches("bytes ")
                .split('-')
                .next()
                .unwrap()
                .parse()
                .unwrap();
            assert_eq!(first, file.len(), "no byte is sent twice");
            file.extend_from_slice(&body);
        }
        if file.len() == SIZE {
            let video = r#"{"id":"Xb7kQ2mN9pA","status":{"uploadStatus":"uploaded"}}"#;
            format!(
                "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{video}",
                video.len()
            )
        } else if file.is_empty() {
            "HTTP/1.1 308 Resume Incomplete\r\nContent-Length: 0\r\n\r\n".to_owned()
        } else {
            format!(
                "HTTP/1.1 308 Resume Incomplete\r\nRange: bytes=0-{}\r\nContent-Length: 0\r\n\r\n",
                file.len() - 1
            )
        }
    } else {
        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_owned()
    };
    stream.write_all(answer.as_bytes()).unwrap();
}

struct Run {
    file: Vec<u8>,
    session: Option<String>,
}

impl UploadRun for Run {
    fn access_token(&mut self) -> Result<SecretText, UploadError> {
        Ok(SecretText::new("ya29.fake-access"))
    }

    fn size(&self) -> u64 {
        self.file.len() as u64
    }

    fn read(&mut self, offset: u64, len: usize) -> Result<Vec<u8>, UploadError> {
        let start = offset as usize;
        Ok(self.file[start..start + len].to_vec())
    }

    fn session(&self) -> Option<String> {
        self.session.clone()
    }

    fn session_started(&mut self, session: &str) -> Result<(), UploadError> {
        self.session = Some(session.to_owned());
        Ok(())
    }

    fn confirmed(&mut self, _bytes: u64) -> Result<(), UploadError> {
        Ok(())
    }

    fn should_stop(&self) -> bool {
        false
    }
}

fn uploader(api: String) -> YouTubeUploader {
    YouTubeUploader::with_transport(UreqTransport::for_uploads_without_proxy(
        Duration::from_secs(10),
    ))
    .with_endpoints(GoogleEndpoints {
        api,
        ..GoogleEndpoints::default()
    })
    .with_chunk_units(1)
}

fn video() -> VideoUpload {
    VideoUpload {
        title: "Probe".into(),
        description: String::new(),
        tags: Vec::new(),
        visibility: Visibility::Private,
        made_for_kids: false,
        synthetic: false,
        publish_at: None,
    }
}

fn file() -> Vec<u8> {
    (0..SIZE).map(|i| (i % 253) as u8).collect()
}

#[test]
fn resume_incomplete_answers_reach_the_adapter_and_the_file_arrives_whole() {
    let youtube = Arc::new(YouTube::default());
    let uploader = uploader(serve(Arc::clone(&youtube)));
    let mut run = Run {
        file: file(),
        session: None,
    };

    let outcome = uploader.upload(&video(), &mut run).unwrap();

    assert_eq!(
        outcome,
        UploadOutcome::Uploaded(UploadedVideo {
            id: "Xb7kQ2mN9pA".into()
        })
    );
    assert_eq!(*youtube.file.lock().unwrap(), run.file);
}

#[test]
fn a_broken_chunk_resumes_from_what_arrived() {
    let youtube = Arc::new(YouTube::default());
    *youtube.fail_first_chunk.lock().unwrap() = true;
    let uploader = uploader(serve(Arc::clone(&youtube)));
    let mut run = Run {
        file: file(),
        session: None,
    };

    let error = uploader.upload(&video(), &mut run).unwrap_err();
    assert!(error.kind.is_transient(), "{error}");
    let session = run.session.clone().expect("the session is kept");

    // The next attempt asks what arrived and sends the rest.
    let outcome = uploader.upload(&video(), &mut run).unwrap();
    assert!(matches!(outcome, UploadOutcome::Uploaded(_)));
    assert_eq!(run.session, Some(session));
    assert_eq!(*youtube.file.lock().unwrap(), run.file);
}
