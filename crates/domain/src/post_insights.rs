//! Post insights (#85; PRD story 90, ADR-0004): what Instagram and TikTok
//! report to a connected account about its own posts. YouTube's owner
//! numbers are `owner_metrics`; these are the other networks', read by the
//! same metrics sync.
//!
//! - **Instagram** (media insights, Facebook Login): per Reel its views,
//!   reach, likes, comments, shares, saves, total interactions, average
//!   watch time and total watch time, one request per post. The data are
//!   up to 48 hours late, and a metric with no data yet is left out of the
//!   answer rather than reported as zero, so every number here is optional
//!   and an empty answer is "no numbers yet". Insights take the media id,
//!   not the shortcode a linked post's address carries: a linked post is
//!   found by listing the account's media and matching their permalinks.
//! - **TikTok** (Display API `video/query`): per video its views, likes,
//!   comments and shares, up to 20 videos per request. Only the creator's
//!   public videos come back; one left out is not found, like a YouTube
//!   post a sync no longer sees. No API gives TikTok watch time or
//!   retention.
//!
//! Neither network reports revenue.

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use crate::{AnalyticsError, AnalyticsErrorKind, Network, PostLink, SecretText};

/// What a network reports of a post beyond its views, likes and comments
/// (part of a metrics snapshot). A number the network did not report is
/// `None`, never zero; YouTube's snapshots have none of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Insights {
    pub shares: Option<u64>,
    /// Instagram: how many times the post was saved.
    pub saves: Option<u64>,
    /// Instagram: how many accounts saw the post at least once.
    pub reach: Option<u64>,
    /// Instagram: likes, saves, comments and shares, minus the ones undone.
    pub interactions: Option<u64>,
    /// Instagram: the average time a play lasted.
    pub average_watch: Option<Duration>,
    /// Instagram: the time the post was played in all, replays included.
    pub watch_time: Option<Duration>,
}

impl Insights {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// A post's numbers as the network answered them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PostNumbers {
    pub views: Option<u64>,
    pub likes: Option<u64>,
    pub comments: Option<u64>,
    pub insights: Insights,
    /// When the post went up, where the answer says (TikTok).
    pub posted_at: Option<SystemTime>,
}

impl PostNumbers {
    /// Whether the answer held no number at all: the network has no data
    /// for the post yet.
    pub fn is_empty(&self) -> bool {
        self.views.is_none()
            && self.likes.is_none()
            && self.comments.is_none()
            && self.insights.is_empty()
    }
}

/// Reads a connected account's own posts on a network with insights
/// (Instagram, TikTok).
pub trait InsightsReader: Send + Sync {
    fn network(&self) -> Network;

    /// The most posts one `read` takes.
    fn batch(&self) -> usize;

    /// The numbers of `posts` (the ids insights take, at most `batch`).
    /// A post the network does not show to the account is left out:
    /// removed, private, or not the account's.
    fn read(
        &self,
        token: &SecretText,
        posts: &[&str],
    ) -> Result<Vec<(String, PostNumbers)>, AnalyticsError>;

    /// One page of the account's media, newest first, after the cursor a
    /// page before gave. Only where a post's link does not carry the id
    /// insights take (Instagram); elsewhere the listing is empty.
    fn media_page(
        &self,
        _token: &SecretText,
        _account: &str,
        _after: Option<&str>,
    ) -> Result<MediaPage, AnalyticsError> {
        Ok(MediaPage::default())
    }
}

/// What a sync learned of one post.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostReading {
    /// The network has the post, with these numbers (all `None` while its
    /// data have not arrived).
    Found(PostNumbers),
    /// The network does not show the post to its account.
    NotFound,
    /// Nothing could be said this time (refused for this post alone): it
    /// keeps what it had.
    Unread,
}

/// Reads `posts` in the reader's batches. A post the network left out is
/// not found; a batch refused for itself (one post's insights, say) reads
/// as unread, or not found when the network says it does not have it.
/// Only what stops the account (token, quota, connection) fails the read.
pub fn read_insights(
    reader: &dyn InsightsReader,
    token: &SecretText,
    posts: &[&str],
) -> Result<Vec<(String, PostReading)>, AnalyticsError> {
    let mut read = Vec::with_capacity(posts.len());
    for batch in posts.chunks(reader.batch().max(1)) {
        match reader.read(token, batch) {
            Ok(found) => read.extend(batch.iter().map(|post| {
                let numbers = found
                    .iter()
                    .find(|(id, _)| id == post)
                    .map(|(_, numbers)| *numbers);
                (
                    (*post).to_owned(),
                    numbers.map_or(PostReading::NotFound, PostReading::Found),
                )
            })),
            Err(error) if error.stops_the_account() => return Err(error),
            Err(error) => {
                let reading = if error.kind == AnalyticsErrorKind::NotFound {
                    PostReading::NotFound
                } else {
                    PostReading::Unread
                };
                read.extend(batch.iter().map(|post| ((*post).to_owned(), reading)));
            }
        }
    }
    Ok(read)
}

/// One of the account's media in a listing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MediaItem {
    /// The id insights take.
    pub id: String,
    pub permalink: Option<String>,
    pub shortcode: Option<String>,
    /// When it went up.
    pub posted_at: Option<SystemTime>,
}

impl MediaItem {
    /// The shortcode a post's link carries: read from the permalink as a
    /// pasted link is, else the listing's own `shortcode`.
    pub fn code(&self) -> Option<String> {
        self.permalink
            .as_deref()
            .and_then(|permalink| PostLink::parse(Network::InstagramReels, permalink).ok())
            .map(|link| link.post_id().to_owned())
            .or_else(|| self.shortcode.clone())
    }
}

/// A page of the account's media and the cursor to the next one.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MediaPage {
    pub items: Vec<MediaItem>,
    pub next: Option<String>,
}

/// The most pages of media one sync lists for an account: with 100 media
/// a page, the newest 2,000.
pub const MEDIA_PAGES: usize = 20;

/// What a listing found for the shortcodes it looked for.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MediaLookup {
    /// Per shortcode, its media.
    pub found: HashMap<String, MediaItem>,
    /// Whether the listing was read to its end: a shortcode still missing
    /// is then not the account's (or no longer there).
    pub complete: bool,
}

impl MediaLookup {
    /// What the lookup says of `code`: its media, not the account's
    /// (`Err(true)`), or unknown past the pages read (`Err(false)`).
    pub fn media(&self, code: &str) -> Result<&MediaItem, bool> {
        self.found.get(code).ok_or(self.complete)
    }
}

/// Finds the media of `codes` (shortcodes from posts' links) by listing
/// `account`'s media, newest first, until every one is found, the listing
/// ends or `max_pages` were read.
pub fn find_media(
    reader: &dyn InsightsReader,
    token: &SecretText,
    account: &str,
    codes: &[&str],
    max_pages: usize,
) -> Result<MediaLookup, AnalyticsError> {
    let mut lookup = MediaLookup::default();
    let mut after: Option<String> = None;
    for _ in 0..max_pages {
        if codes.iter().all(|code| lookup.found.contains_key(*code)) {
            return Ok(lookup);
        }
        let page = reader.media_page(token, account, after.as_deref())?;
        for item in page.items {
            if let Some(code) = item.code().filter(|code| codes.contains(&code.as_str())) {
                lookup.found.entry(code).or_insert(item);
            }
        }
        match page.next {
            // A cursor that does not move would list the same page again.
            Some(next) if after.as_deref() != Some(next.as_str()) => after = Some(next),
            _ => {
                lookup.complete = true;
                return Ok(lookup);
            }
        }
    }
    Ok(lookup)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    fn numbers(views: u64) -> PostNumbers {
        PostNumbers {
            views: Some(views),
            likes: Some(views / 10),
            ..PostNumbers::default()
        }
    }

    #[test]
    fn an_answer_without_any_number_is_no_data_not_zeros() {
        assert!(PostNumbers::default().is_empty());
        let zero = PostNumbers {
            views: Some(0),
            ..PostNumbers::default()
        };
        assert!(!zero.is_empty(), "a reported zero is a number");
        let shares_only = PostNumbers {
            insights: Insights {
                shares: Some(0),
                ..Insights::default()
            },
            ..PostNumbers::default()
        };
        assert!(!shares_only.is_empty());
        assert!(Insights::default().is_empty());
    }

    /// Answers from what a test set and keeps every call.
    #[derive(Default)]
    struct Fake {
        batch: usize,
        posts: HashMap<String, PostNumbers>,
        /// Per post id, what reading it fails with.
        refused: HashMap<String, AnalyticsError>,
        pages: Vec<MediaPage>,
        reads: Mutex<Vec<Vec<String>>>,
        listed: Mutex<Vec<Option<String>>>,
    }

    impl InsightsReader for Fake {
        fn network(&self) -> Network {
            Network::InstagramReels
        }

        fn batch(&self) -> usize {
            self.batch
        }

        fn read(
            &self,
            _: &SecretText,
            posts: &[&str],
        ) -> Result<Vec<(String, PostNumbers)>, AnalyticsError> {
            self.reads
                .lock()
                .unwrap()
                .push(posts.iter().map(|p| (*p).to_owned()).collect());
            if let Some(error) = posts.iter().find_map(|p| self.refused.get(*p)) {
                return Err(error.clone());
            }
            Ok(posts
                .iter()
                .filter_map(|p| Some(((*p).to_owned(), *self.posts.get(*p)?)))
                .collect())
        }

        fn media_page(
            &self,
            _: &SecretText,
            account: &str,
            after: Option<&str>,
        ) -> Result<MediaPage, AnalyticsError> {
            assert_eq!(account, "17841400000000001");
            self.listed.lock().unwrap().push(after.map(str::to_owned));
            let ix = after.map_or(0, |cursor| cursor.parse::<usize>().unwrap());
            Ok(self.pages.get(ix).cloned().unwrap_or_default())
        }
    }

    fn token() -> SecretText {
        SecretText::new("EAAG.page-token")
    }

    #[test]
    fn posts_are_read_in_batches_and_one_left_out_is_not_found() {
        let fake = Fake {
            batch: 2,
            posts: HashMap::from([("1".to_owned(), numbers(10)), ("3".to_owned(), numbers(0))]),
            ..Fake::default()
        };
        let read = read_insights(&fake, &token(), &["1", "2", "3"]).unwrap();
        assert_eq!(
            read,
            [
                ("1".to_owned(), PostReading::Found(numbers(10))),
                ("2".to_owned(), PostReading::NotFound),
                ("3".to_owned(), PostReading::Found(numbers(0))),
            ]
        );
        assert_eq!(fake.reads.lock().unwrap().len(), 2, "20 a call on TikTok");
    }

    #[test]
    fn a_post_refused_alone_is_unread_or_not_found_and_the_token_stops_all() {
        let fake = Fake {
            batch: 1,
            posts: HashMap::from([("1".to_owned(), numbers(5))]),
            refused: HashMap::from([
                (
                    "2".to_owned(),
                    AnalyticsError::new(AnalyticsErrorKind::Forbidden, "(#10) Not enough viewers"),
                ),
                (
                    "3".to_owned(),
                    AnalyticsError::new(AnalyticsErrorKind::NotFound, "(#100) does not exist"),
                ),
            ]),
            ..Fake::default()
        };
        let read = read_insights(&fake, &token(), &["2", "1", "3"]).unwrap();
        assert_eq!(read[0].1, PostReading::Unread);
        assert_eq!(read[1].1, PostReading::Found(numbers(5)));
        assert_eq!(read[2].1, PostReading::NotFound);

        let signed_out = Fake {
            batch: 1,
            refused: HashMap::from([(
                "1".to_owned(),
                AnalyticsError::new(AnalyticsErrorKind::SignedOut, "(#190)"),
            )]),
            ..Fake::default()
        };
        let error = read_insights(&signed_out, &token(), &["1", "2"]).unwrap_err();
        assert_eq!(error.kind, AnalyticsErrorKind::SignedOut);
        assert_eq!(signed_out.reads.lock().unwrap().len(), 1, "stops at once");
    }

    fn item(id: &str, permalink: Option<&str>, shortcode: Option<&str>) -> MediaItem {
        MediaItem {
            id: id.into(),
            permalink: permalink.map(str::to_owned),
            shortcode: shortcode.map(str::to_owned),
            posted_at: None,
        }
    }

    #[test]
    fn a_media_reads_its_shortcode_off_the_permalink_as_a_pasted_link() {
        let reel = item(
            "1",
            Some("https://www.instagram.com/reel/DAbCdEfGhIj/"),
            Some("ignored"),
        );
        assert_eq!(reel.code().as_deref(), Some("DAbCdEfGhIj"));
        let post = item("2", Some("https://www.instagram.com/p/C9xYz12AbCd/"), None);
        assert_eq!(post.code().as_deref(), Some("C9xYz12AbCd"));
        let bare = item("3", None, Some("C1aaaaaaaaa"));
        assert_eq!(bare.code().as_deref(), Some("C1aaaaaaaaa"));
        let odd = item("4", Some("https://example.com/x"), None);
        assert_eq!(odd.code(), None);
    }

    const ACCOUNT: &str = "17841400000000001";

    fn listing() -> Fake {
        Fake {
            pages: vec![
                MediaPage {
                    items: vec![
                        item(
                            "100",
                            Some("https://www.instagram.com/reel/AAAAAAAAAAA/"),
                            None,
                        ),
                        item(
                            "101",
                            Some("https://www.instagram.com/reel/BBBBBBBBBBB/"),
                            None,
                        ),
                    ],
                    next: Some("1".into()),
                },
                MediaPage {
                    items: vec![item(
                        "102",
                        Some("https://www.instagram.com/p/CCCCCCCCCCC/"),
                        None,
                    )],
                    next: None,
                },
            ],
            ..Fake::default()
        }
    }

    #[test]
    fn shortcodes_map_to_media_by_listing_until_all_are_found() {
        let fake = listing();
        let lookup = find_media(&fake, &token(), ACCOUNT, &["BBBBBBBBBBB"], MEDIA_PAGES).unwrap();
        assert_eq!(lookup.media("BBBBBBBBBBB").unwrap().id, "101");
        assert_eq!(*fake.listed.lock().unwrap(), [None], "stops once found");

        let fake = listing();
        let lookup = find_media(
            &fake,
            &token(),
            ACCOUNT,
            &["CCCCCCCCCCC", "AAAAAAAAAAA", "ZZZZZZZZZZZ"],
            MEDIA_PAGES,
        )
        .unwrap();
        assert_eq!(lookup.media("AAAAAAAAAAA").unwrap().id, "100");
        assert_eq!(lookup.media("CCCCCCCCCCC").unwrap().id, "102");
        assert!(lookup.complete);
        assert_eq!(
            lookup.media("ZZZZZZZZZZZ"),
            Err(true),
            "the whole listing read: not the account's"
        );
        assert_eq!(*fake.listed.lock().unwrap(), [None, Some("1".to_owned())]);
    }

    #[test]
    fn a_listing_cut_short_says_nothing_of_what_it_did_not_reach() {
        let fake = listing();
        let lookup = find_media(&fake, &token(), ACCOUNT, &["CCCCCCCCCCC"], 1).unwrap();
        assert!(!lookup.complete);
        assert_eq!(lookup.media("CCCCCCCCCCC"), Err(false));
    }

    #[test]
    fn a_cursor_that_does_not_move_ends_the_listing() {
        let fake = Fake {
            pages: vec![
                MediaPage::default(),
                MediaPage {
                    items: Vec::new(),
                    next: Some("1".into()),
                },
            ],
            ..Fake::default()
        };
        // Page 0 is empty with no cursor: the end.
        let lookup = find_media(&fake, &token(), ACCOUNT, &["AAAAAAAAAAA"], 5).unwrap();
        assert!(lookup.complete);
        let stuck = Fake {
            pages: vec![
                MediaPage {
                    items: Vec::new(),
                    next: Some("1".into()),
                },
                MediaPage {
                    items: Vec::new(),
                    next: Some("1".into()),
                },
            ],
            ..Fake::default()
        };
        let lookup = find_media(&stuck, &token(), ACCOUNT, &["AAAAAAAAAAA"], 5).unwrap();
        assert!(lookup.complete);
        assert_eq!(stuck.listed.lock().unwrap().len(), 2);
    }
}
