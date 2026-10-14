//! Bardo's release feed against recorded responses (see
//! `fixtures/github-releases/README.md`). No test calls GitHub.

mod common;

use std::time::{Duration, SystemTime};

use bardo_ai::GitHubReleases;
use bardo_ai::http::{HttpResponse, Method};
use bardo_domain::{DownloadError, ReleaseFeed, UpdateCheckError, Version};
use common::{Scripted, fixture};

const INSTALLER: &str =
    "https://github.com/fernandolisboa/bardo/releases/download/v0.2.0/Bardo-Setup-0.2.0-x64.exe";

fn feed(api: &[&str]) -> GitHubReleases<Scripted, Scripted> {
    GitHubReleases::with_transports(
        Scripted::new(
            api.iter()
                .map(|name| fixture("github-releases", name))
                .collect(),
        ),
        Scripted::default(),
    )
}

fn downloads(answers: Vec<HttpResponse>) -> GitHubReleases<Scripted, Scripted> {
    GitHubReleases::with_transports(Scripted::default(), Scripted::new(answers))
}

#[test]
fn the_latest_release_reads_its_version_notes_and_files() {
    let feed = feed(&["latest"]);
    let release = feed.latest().unwrap();
    assert_eq!(release.version, Version::new(0, 2, 0));
    assert_eq!(
        release.page,
        "https://github.com/fernandolisboa/bardo/releases/tag/v0.2.0"
    );
    assert!(release.notes.starts_with("## What's Changed"));
    assert_eq!(
        release.published_at,
        Some(humantime::parse_rfc3339("2026-11-02T15:24:02Z").unwrap())
    );
    let installer = release.installer().unwrap();
    assert_eq!(installer.url, INSTALLER);
    assert_eq!(installer.size, 48_213_504);
    assert_eq!(
        installer.sha256.as_deref(),
        Some("9f2c7a4b1e8d3c6f5a0b2d4e6f8a1c3e5b7d9f0a2c4e6b8d1f3a5c7e9b0d2f4a")
    );
    assert_eq!(
        release.checksum().unwrap().url,
        format!("{INSTALLER}.sha256")
    );

    let sent = feed.transport().sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].method, Method::Get);
    assert_eq!(
        sent[0].url,
        "https://api.github.com/repos/fernandolisboa/bardo/releases/latest"
    );
    assert_eq!(
        sent[0].header("accept"),
        Some("application/vnd.github+json")
    );
    assert_eq!(sent[0].header("x-github-api-version"), Some("2022-11-28"));
    assert_eq!(sent[0].header("authorization"), None, "no token");
}

#[test]
fn a_release_without_files_still_reads() {
    let release = feed(&["latest-no-installer"]).latest().unwrap();
    assert_eq!(release.version, Version::new(0, 2, 0));
    assert!(release.installer().is_none());
    assert!(release.checksum().is_none());
}

#[test]
fn nothing_published_is_no_release() {
    assert_eq!(feed(&["none"]).latest(), Err(UpdateCheckError::NoRelease));
}

#[test]
fn a_tag_that_is_not_a_version_is_an_unexpected_answer() {
    assert!(matches!(
        feed(&["latest-not-a-version"]).latest(),
        Err(UpdateCheckError::Unexpected(why)) if why.contains("nightly")
    ));
}

#[test]
fn the_hourly_limit_says_when_it_resets() {
    assert_eq!(
        feed(&["rate-limited"]).latest(),
        Err(UpdateCheckError::RateLimited {
            until: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_793_638_800)),
        })
    );
}

#[test]
fn a_secondary_limit_waits_as_long_as_it_asks() {
    let before = SystemTime::now();
    let Err(UpdateCheckError::RateLimited { until: Some(until) }) =
        feed(&["secondary-rate-limit"]).latest()
    else {
        panic!("rate limited");
    };
    let wait = until.duration_since(before).unwrap();
    assert!(
        wait >= Duration::from_secs(60) && wait < Duration::from_secs(70),
        "{wait:?}"
    );
}

#[test]
fn a_refusal_that_is_not_a_limit_is_unexpected() {
    assert!(matches!(
        feed(&["forbidden"]).latest(),
        Err(UpdateCheckError::Unexpected(_))
    ));
}

#[test]
fn no_answer_or_a_server_error_is_offline() {
    let offline = GitHubReleases::with_transports(Scripted::offline(), Scripted::default());
    assert!(matches!(
        offline.latest(),
        Err(UpdateCheckError::Offline(why)) if why.contains("dns")
    ));
    assert!(matches!(
        feed(&["unavailable"]).latest(),
        Err(UpdateCheckError::Offline(_))
    ));
}

#[test]
fn the_checksum_file_is_read_as_text() {
    let line = format!("{}  Bardo-Setup-0.2.0-x64.exe\n", "a".repeat(64));
    let feed = GitHubReleases::with_transports(
        Scripted::new(vec![HttpResponse::new(200, line.clone())]),
        Scripted::default(),
    );
    assert_eq!(
        feed.text(&format!("{INSTALLER}.sha256"), 4096).unwrap(),
        line
    );
}

#[test]
fn the_installer_streams_with_its_progress() {
    let bytes = vec![7u8; 150_000];
    let feed = downloads(vec![HttpResponse::new(
        200,
        String::from_utf8(bytes.clone()).unwrap(),
    )]);
    let mut out = Vec::new();
    let mut seen = Vec::new();
    let written = feed
        .download(INSTALLER, &mut out, 1_000_000, &mut |done| {
            seen.push(done);
            true
        })
        .unwrap();
    assert_eq!(written, 150_000);
    assert_eq!(out, bytes);
    assert_eq!(seen.first(), Some(&0));
    assert_eq!(seen.last(), Some(&150_000));
    assert!(seen.windows(2).all(|pair| pair[0] <= pair[1]));
    assert_eq!(feed.downloads().sent()[0].url, INSTALLER);
}

#[test]
fn a_download_stops_when_told_to() {
    let feed = downloads(vec![HttpResponse::new(200, "x".repeat(200_000))]);
    let mut out = Vec::new();
    let mut calls = 0;
    let stopped = feed.download(INSTALLER, &mut out, 1_000_000, &mut |_| {
        calls += 1;
        calls < 2
    });
    assert_eq!(stopped, Err(DownloadError::Stopped));
    assert!(out.len() < 200_000);
}

#[test]
fn a_file_over_its_limit_is_refused() {
    let feed = downloads(vec![HttpResponse::new(200, "x".repeat(1000))]);
    let result = feed.download(INSTALLER, &mut Vec::new(), 999, &mut |_| true);
    assert_eq!(result, Err(DownloadError::TooBig(999)));
}

#[test]
fn an_error_answer_is_never_written_as_the_file() {
    let feed = downloads(vec![
        HttpResponse::new(404, "Not Found"),
        HttpResponse::new(502, "Bad gateway"),
    ]);
    let mut out = Vec::new();
    assert!(matches!(
        feed.download(INSTALLER, &mut out, 1000, &mut |_| true),
        Err(DownloadError::Unexpected(_))
    ));
    assert!(matches!(
        feed.download(INSTALLER, &mut out, 1000, &mut |_| true),
        Err(DownloadError::Unreachable(_))
    ));
    assert!(out.is_empty());
}

#[test]
fn only_this_repositorys_release_files_are_fetched() {
    let feed = downloads(Vec::new());
    for url in [
        "https://example.test/Bardo-Setup-0.2.0-x64.exe",
        "http://github.com/fernandolisboa/bardo/releases/download/v0.2.0/a.exe",
        "https://github.com/someone/bardo/releases/download/v0.2.0/a.exe",
        "https://github.com/fernandolisboa/bardo/releases/download/../../x/a.exe",
        "https://github.com/fernandolisboa/bardo/releases/download/v0.2.0/a.exe?x=1",
        "https://github.com/fernandolisboa/bardo/releases/download/v0.2.0/",
        "https://github.com/fernandolisboa/bardo/releases/download/a.exe",
        "https://github.com/fernandolisboa/bardo/releases/download.evil.test/v0.2.0/a.exe",
    ] {
        assert!(
            matches!(
                feed.download(url, &mut Vec::new(), 10, &mut |_| true),
                Err(DownloadError::NotAReleaseFile(_))
            ),
            "{url}"
        );
        assert!(
            matches!(feed.text(url, 10), Err(DownloadError::NotAReleaseFile(_))),
            "{url}"
        );
    }
    assert!(feed.downloads().sent().is_empty());
    assert!(feed.transport().sent().is_empty());
}
