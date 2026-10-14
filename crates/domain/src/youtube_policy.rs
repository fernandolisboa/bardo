//! What the YouTube API Services Developer Policies ask of Bardo (issue
//! #145, ADR-0012): the privacy policy the user accepts before Bardo uses
//! YouTube, and how long data read from YouTube may stay in the database.
//!
//! The retention rule: nothing read from YouTube is kept more than
//! [`KEEP_AT_MOST`], except the numbers of a channel's own posts while
//! Bardo holds that channel's authorization and has confirmed it within
//! that time. Numbers Bardo works out from YouTube data (research scores,
//! theme rankings) go with the data they came from. A daily upkeep
//! deletes what the rule no longer allows, so it deletes anything that
//! would pass the limit before the next upkeep.

use std::time::{Duration, SystemTime};

use crate::{NetworkAccountId, NetworkConnection, ProfileId, RepositoryError};

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// The privacy policy's version. Raised when Bardo starts using YouTube
/// data in a way the accepted version did not describe, so the user is
/// asked to accept it again (policy III.D.1).
pub const PRIVACY_POLICY_VERSION: u32 = 1;

/// The privacy policy the user accepted, and when.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrivacyConsent {
    pub version: u32,
    pub accepted_at: SystemTime,
}

impl PrivacyConsent {
    /// The current policy, accepted `at`.
    pub fn current(at: SystemTime) -> Self {
        Self {
            version: PRIVACY_POLICY_VERSION,
            accepted_at: at,
        }
    }

    /// Whether it is the policy Bardo has now.
    pub fn is_current(&self) -> bool {
        self.version == PRIVACY_POLICY_VERSION
    }
}

/// The longest YouTube data may be kept, and how long a channel's
/// authorization may go unconfirmed (policies III.E.4 and III.D.2).
pub const KEEP_AT_MOST: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// How often the upkeep runs while Bardo or its background agent is up.
pub const UPKEEP_EVERY: Duration = DAY;

/// How old a channel's post numbers may get before a start reads them
/// again, whatever the automatic sync setting, so they are refreshed
/// rather than deleted.
pub const REFRESH_AFTER: Duration = Duration::from_secs(25 * 24 * 60 * 60);

/// The oldest YouTube data an upkeep at `now` keeps: anything older would
/// pass [`KEEP_AT_MOST`] before the next upkeep.
pub fn keep_since(now: SystemTime) -> SystemTime {
    now.checked_sub(KEEP_AT_MOST - UPKEEP_EVERY)
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// When the network last confirmed the connection's authorization: its
/// last accepted refresh, or the connection itself.
pub fn confirmed_at(connection: &NetworkConnection) -> SystemTime {
    connection
        .refreshed_at
        .map_or(connection.connected_at, |refreshed| {
            refreshed.max(connection.connected_at)
        })
}

/// Whether an upkeep at `now` may keep what Bardo read through the
/// connection.
pub fn authorization_current(connection: &NetworkConnection, now: SystemTime) -> bool {
    confirmed_at(connection) >= keep_since(now)
}

/// What one upkeep keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YouTubeRetention {
    /// See [`keep_since`].
    pub keep_since: SystemTime,
    /// The YouTube accounts whose authorization is current: their posts'
    /// numbers stay whatever their age.
    pub authorized: Vec<NetworkAccountId>,
}

impl YouTubeRetention {
    pub fn at(now: SystemTime, authorized: Vec<NetworkAccountId>) -> Self {
        Self {
            keep_since: keep_since(now),
            authorized,
        }
    }
}

/// What a deletion removed, for the log and the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Forgotten {
    /// Niche research results.
    pub research: usize,
    /// Theme rankings cleared.
    pub rankings: usize,
    /// Post snapshots deleted.
    pub snapshots: usize,
    /// Snapshots that lost the owner's numbers (YouTube Analytics).
    pub owner_numbers: usize,
    /// Posts that lost their retention curve.
    pub curves: usize,
}

impl Forgotten {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

impl std::ops::Add for Forgotten {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            research: self.research + other.research,
            rankings: self.rankings + other.rankings,
            snapshots: self.snapshots + other.snapshots,
            owner_numbers: self.owner_numbers + other.owner_numbers,
            curves: self.curves + other.curves,
        }
    }
}

/// Deletes stored YouTube data. Bardo's own records stay: the channels,
/// themes, projects, the address of each post and what Bardo uploaded.
pub trait YouTubeDataStore: Send + Sync {
    /// Deletes the owner's YouTube data `rule` no longer allows: niche
    /// research fetched and theme rankings made before `keep_since`; the
    /// numbers of YouTube posts taken before it, unless the post's account
    /// is authorized; the owner's numbers and retention curves of posts
    /// whose account is not; and every number of a post YouTube has not
    /// shown since before `keep_since`.
    fn forget_expired(
        &self,
        owner: ProfileId,
        rule: &YouTubeRetention,
    ) -> Result<Forgotten, RepositoryError>;

    /// Deletes what Bardo read through `account`'s authorization: the
    /// owner's numbers and retention curves of its YouTube posts, and
    /// their numbers taken before `keep_since`.
    fn forget_account(
        &self,
        account: NetworkAccountId,
        keep_since: SystemTime,
    ) -> Result<Forgotten, RepositoryError>;

    /// Deletes every YouTube datum the owner has: research, rankings, and
    /// the numbers and curves of their YouTube posts.
    fn forget_all(&self, owner: ProfileId) -> Result<Forgotten, RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ConnectedIdentity, ConnectionStatus};

    fn at(days: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_000 * 86_400) + DAY * days as u32
    }

    fn connection(connected: SystemTime, refreshed: Option<SystemTime>) -> NetworkConnection {
        NetworkConnection {
            account: NetworkAccountId::new(),
            owner: ProfileId::new(),
            status: ConnectionStatus::Connected,
            identity: ConnectedIdentity {
                id: "UC123".into(),
                name: "Channel".into(),
            },
            scopes: Vec::new(),
            expires_at: connected,
            connected_at: connected,
            refreshed_at: refreshed,
        }
    }

    #[test]
    fn an_upkeep_keeps_what_stays_within_thirty_days_until_the_next() {
        let now = at(100);
        assert_eq!(keep_since(now), at(71));
        // Kept today, it is 30 days old at the next upkeep: no older.
        assert_eq!(
            (now + UPKEEP_EVERY).duration_since(keep_since(now)).ok(),
            Some(KEEP_AT_MOST)
        );
    }

    #[test]
    fn a_connection_is_confirmed_by_its_last_refresh_or_its_start() {
        assert_eq!(confirmed_at(&connection(at(10), None)), at(10));
        assert_eq!(confirmed_at(&connection(at(10), Some(at(40)))), at(40));
        // Reconnected after the last refresh of the earlier connection.
        assert_eq!(confirmed_at(&connection(at(50), Some(at(40)))), at(50));
    }

    #[test]
    fn authorization_lapses_once_confirmed_before_the_kept_window() {
        let now = at(100);
        assert!(authorization_current(&connection(at(71), None), now));
        assert!(!authorization_current(
            &connection(at(71) - Duration::from_secs(1), None),
            now
        ));
        assert!(authorization_current(&connection(at(1), Some(at(99))), now));
        assert!(!authorization_current(
            &connection(at(1), Some(at(70))),
            now
        ));
    }

    #[test]
    fn only_the_current_policy_counts_as_accepted() {
        assert!(PrivacyConsent::current(at(1)).is_current());
        let older = PrivacyConsent {
            version: PRIVACY_POLICY_VERSION - 1,
            accepted_at: at(1),
        };
        assert!(!older.is_current());
    }

    #[test]
    fn deletions_add_up() {
        let one = Forgotten {
            research: 1,
            snapshots: 2,
            ..Forgotten::default()
        };
        let two = Forgotten {
            snapshots: 3,
            curves: 1,
            ..Forgotten::default()
        };
        assert!(Forgotten::default().is_empty());
        assert_eq!(
            one + two,
            Forgotten {
                research: 1,
                snapshots: 5,
                curves: 1,
                ..Forgotten::default()
            }
        );
    }
}
