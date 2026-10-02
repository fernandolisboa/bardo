//! YouTube market data against recorded responses (see
//! `fixtures/market-data/README.md`). No test calls YouTube.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::SystemTime;

use bardo_ai::YouTubeMarketData;
use bardo_ai::http::{HttpRequest, HttpResponse, Transport, TransportError};
use bardo_ai::market::QUOTA_UNITS_PER_NICHE;
use bardo_domain::{
    ApiKey, ContentLanguage, Country, Market, MarketData, Niche, Provider, ProviderFailureKind,
    UploadSample,
};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/market-data");
const KEY: &str = "AIzaSyTestKey0001abcdefghijklmnopqrstu";

/// A recorded response.
fn fixture(name: &str) -> HttpResponse {
    let path = Path::new(FIXTURES).join(format!("{name}.http"));
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    HttpResponse::from_recording(&raw).expect("a recorded response")
}

/// Answers each API resource (`search`, `videos`, `channels`) with a
/// fixture and remembers every request.
#[derive(Default)]
struct Replay {
    fixtures: HashMap<&'static str, &'static str>,
    sent: Mutex<Vec<(String, Option<String>)>>,
}

impl Replay {
    fn new(fixtures: &[(&'static str, &'static str)]) -> Self {
        Self {
            fixtures: fixtures.iter().copied().collect(),
            ..Self::default()
        }
    }

    fn urls(&self) -> Vec<String> {
        self.sent
            .lock()
            .unwrap()
            .iter()
            .map(|(url, _)| url.clone())
            .collect()
    }
}

impl Transport for Replay {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError> {
        let key = request.header_value("x-goog-api-key").map(str::to_owned);
        self.sent.lock().unwrap().push((request.url.clone(), key));
        let resource = request
            .url
            .strip_prefix("https://www.googleapis.com/youtube/v3/")
            .and_then(|rest| rest.split('?').next())
            .expect("a YouTube Data API URL");
        let name = self
            .fixtures
            .get(resource)
            .unwrap_or_else(|| panic!("no fixture for {resource}"));
        Ok(fixture(name))
    }
}

struct Offline;

impl Transport for Offline {
    fn send(&self, _: &HttpRequest) -> Result<HttpResponse, TransportError> {
        Err(TransportError("dns error: no such host".into()))
    }
}

fn key() -> ApiKey {
    ApiKey::parse(Provider::YouTubeData, KEY).unwrap()
}

fn since() -> SystemTime {
    humantime::parse_rfc3339("2026-09-02T00:00:00Z").unwrap()
}

fn at(text: &str) -> SystemTime {
    humantime::parse_rfc3339(text).unwrap()
}

fn us() -> Market {
    Market::new(Country::UnitedStates, ContentLanguage::English)
}

fn space_history() -> Replay {
    Replay::new(&[
        ("search", "search-space-history"),
        ("videos", "videos-space-history"),
        ("channels", "channels-space-history"),
    ])
}

fn query(url: &str) -> HashMap<String, String> {
    let (_, query) = url.split_once('?').unwrap();
    form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect()
}

#[test]
fn recent_uploads_join_views_and_channel_sizes() {
    let data = YouTubeMarketData::with_transport(space_history());
    let niche = Niche::new("space history").unwrap();

    let sample = data.recent_uploads(&key(), &niche, us(), since()).unwrap();

    assert_eq!(sample.upload_volume, 48_213);
    let upload = |channel: &str, published: &str, views, subscribers| UploadSample {
        channel_id: channel.into(),
        published_at: at(published),
        views,
        channel_subscribers: subscribers,
    };
    assert_eq!(
        sample.uploads,
        [
            upload(
                "UCa1a1a1a1a1a1a1a1a1a1a1",
                "2026-09-27T14:00:08Z",
                125_431,
                Some(1_840_000)
            ),
            upload(
                "UCb2b2b2b2b2b2b2b2b2b2b2",
                "2026-09-21T18:30:00Z",
                8_790,
                Some(6_300)
            ),
            upload(
                "UCa1a1a1a1a1a1a1a1a1a1a1",
                "2026-09-30T09:15:42Z",
                40_102,
                Some(1_840_000)
            ),
            // The channel hides its subscriber count.
            upload(
                "UCc3c3c3c3c3c3c3c3c3c3c3",
                "2026-09-11T22:04:19Z",
                2_210,
                None
            ),
            // The fifth hit has no statistics (removed meanwhile): left out.
        ]
    );
}

#[test]
fn search_is_scoped_to_the_market_window_and_query() {
    let replay = space_history();
    let data = YouTubeMarketData::with_transport(replay);
    let niche = Niche::new("história do espaço & NASA").unwrap();
    let brazil = Market::new(Country::Brazil, ContentLanguage::Portuguese);

    data.recent_uploads(&key(), &niche, brazil, since())
        .unwrap();

    let urls = data.transport().urls();
    let search = query(&urls[0]);
    assert_eq!(search["q"], "história do espaço & NASA");
    assert_eq!(search["regionCode"], "BR");
    assert_eq!(search["relevanceLanguage"], "pt");
    assert_eq!(search["publishedAfter"], "2026-09-02T00:00:00Z");
    assert_eq!(search["type"], "video");
    assert_eq!(search["order"], "relevance");
    assert_eq!(search["maxResults"], "50");
    assert!(search.contains_key("fields"), "asks only for what it reads");
}

#[test]
fn ids_are_looked_up_in_one_call_each_with_channels_once() {
    let data = YouTubeMarketData::with_transport(space_history());
    data.recent_uploads(&key(), &Niche::new("space history").unwrap(), us(), since())
        .unwrap();

    let urls = data.transport().urls();
    assert_eq!(urls.len(), 3);
    assert!(urls[1].starts_with("https://www.googleapis.com/youtube/v3/videos?"));
    assert_eq!(
        query(&urls[1])["id"],
        "Qm4f1rT8vXa,Lp2c9Wk3sYb,Zt7h3Nq5dRc,Hy6j8Pm1gFd,Wv5k2Bx9nTe"
    );
    assert!(urls[2].starts_with("https://www.googleapis.com/youtube/v3/channels?"));
    assert_eq!(
        query(&urls[2])["id"],
        "UCa1a1a1a1a1a1a1a1a1a1a1,UCb2b2b2b2b2b2b2b2b2b2b2,UCc3c3c3c3c3c3c3c3c3c3c3,UCd4d4d4d4d4d4d4d4d4d4d4"
    );
}

#[test]
fn the_key_travels_in_a_header_never_in_the_url() {
    let data = YouTubeMarketData::with_transport(space_history());
    data.recent_uploads(&key(), &Niche::new("space history").unwrap(), us(), since())
        .unwrap();

    for (url, header) in data.transport().sent.lock().unwrap().iter() {
        assert!(!url.contains(KEY), "{url}");
        assert_eq!(header.as_deref(), Some(KEY));
    }
}

#[test]
fn a_search_without_hits_costs_one_call() {
    let data = YouTubeMarketData::with_transport(Replay::new(&[("search", "search-empty")]));
    let sample = data
        .recent_uploads(&key(), &Niche::new("zzz").unwrap(), us(), since())
        .unwrap();
    assert_eq!(sample.upload_volume, 0);
    assert!(sample.uploads.is_empty());
    assert_eq!(data.transport().urls().len(), 1);
}

#[test]
fn failures_are_classified_with_the_providers_message() {
    let cases = [
        (
            "search-key-rejected",
            ProviderFailureKind::Rejected,
            "API key not valid. Please pass a valid API key.",
        ),
        (
            "search-quota",
            ProviderFailureKind::LimitReached,
            "exceeded your",
        ),
        (
            "search-api-disabled",
            ProviderFailureKind::NotAllowed,
            "YouTube Data API v3 has not been used",
        ),
        (
            "search-backend-error",
            ProviderFailureKind::ProviderDown,
            "The service is currently unavailable.",
        ),
        ("search-html", ProviderFailureKind::Unexpected, "unreadable"),
    ];
    for (fixture_name, kind, message) in cases {
        let data = YouTubeMarketData::with_transport(Replay::new(&[("search", fixture_name)]));
        let failure = data
            .recent_uploads(&key(), &Niche::new("space").unwrap(), us(), since())
            .unwrap_err();
        assert_eq!(failure.kind, kind, "{fixture_name}");
        assert!(
            failure.detail.contains(message),
            "{fixture_name}: {}",
            failure.detail
        );
        assert_eq!(
            data.transport().urls().len(),
            1,
            "stops at the first failure"
        );
    }
}

#[test]
fn no_answer_is_unreachable() {
    let data = YouTubeMarketData::with_transport(Offline);
    let failure = data
        .recent_uploads(&key(), &Niche::new("space").unwrap(), us(), since())
        .unwrap_err();
    assert_eq!(failure.kind, ProviderFailureKind::Unreachable);
    assert_eq!(failure.detail, "dns error: no such host");
}

#[test]
fn a_niche_costs_a_search_and_two_lookups_of_quota() {
    assert_eq!(QUOTA_UNITS_PER_NICHE, 102);
    assert_eq!(
        YouTubeMarketData::with_transport(Offline).quota_units_per_niche(),
        102
    );
}
