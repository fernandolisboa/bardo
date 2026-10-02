//! A channel's account on one network: its handle, the metadata every
//! video for it starts from, and the render preset values it overrides.
//! Credentials arrive with publishing (ADR-0003).

use std::collections::HashSet;
use std::sync::Arc;

use crate::{
    AspectRatio, ChannelId, ContentLanguage, Network, PresetOverrides, ProfileId, RenderPreset,
    RepositoryError, Resolution, VideoCodec, Visibility,
};

uuid_id!(
    /// Identifies a network account.
    NetworkAccountId
);

/// Raw account fields as the user typed them, before validation. Preset
/// values are text so the domain parses them; an empty one keeps the
/// network's built-in value, as does `None` for the choices.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NetworkAccountDraft {
    pub handle: String,
    /// `None` uses the channel's content language.
    pub language: Option<ContentLanguage>,
    pub tags: Vec<String>,
    pub description_footer: String,
    pub visibility: Visibility,
    pub aspect: Option<AspectRatio>,
    pub resolution: Option<Resolution>,
    pub codec: Option<VideoCodec>,
    /// Mbps, e.g. `12` or `7.5`.
    pub bitrate: String,
    /// `m:ss`, `h:mm:ss` or seconds.
    pub max_duration: String,
    /// LUFS, e.g. `-14`.
    pub loudness: String,
}

impl NetworkAccountDraft {
    /// The preset a render would use if this draft were saved for
    /// `network`, for previews while the user types. Values that do not
    /// parse yet keep the network's built-in ones.
    pub fn render_preset(&self, network: Network) -> RenderPreset {
        network.render_preset().with(&PresetOverrides {
            aspect: self.aspect,
            resolution: self.resolution,
            codec: self.codec,
            bitrate: self.bitrate.parse().ok(),
            max_duration: self.max_duration.parse().ok(),
            loudness: self.loudness.parse().ok(),
        })
    }
}

/// Why a draft is not a valid account. One entry per offending field, so a
/// form can show every problem at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NetworkAccountFieldError {
    HandleRequired,
    HandleTooLong,
    /// Spaces, links or an `@` past the first character.
    HandleInvalid,
    TooManyTags,
    TagTooLong,
    DescriptionFooterTooLong,
    /// The network does not offer this visibility.
    VisibilityNotOffered,
    BitrateInvalid,
    MaxDurationInvalid,
    LoudnessInvalid,
}

impl NetworkAccountFieldError {
    pub const ALL: [NetworkAccountFieldError; 10] = [
        NetworkAccountFieldError::HandleRequired,
        NetworkAccountFieldError::HandleTooLong,
        NetworkAccountFieldError::HandleInvalid,
        NetworkAccountFieldError::TooManyTags,
        NetworkAccountFieldError::TagTooLong,
        NetworkAccountFieldError::DescriptionFooterTooLong,
        NetworkAccountFieldError::VisibilityNotOffered,
        NetworkAccountFieldError::BitrateInvalid,
        NetworkAccountFieldError::MaxDurationInvalid,
        NetworkAccountFieldError::LoudnessInvalid,
    ];
}

/// What every video's metadata for this account starts from. The metadata
/// itself (title, description, tags) is per video, generated and editable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataDefaults {
    language: Option<ContentLanguage>,
    tags: Vec<String>,
    description_footer: String,
    visibility: Visibility,
}

impl MetadataDefaults {
    /// `None` when the account follows the channel's content language.
    pub fn language(&self) -> Option<ContentLanguage> {
        self.language
    }

    /// Without `#`; each network's export adds its own syntax.
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    /// Text appended to every description, such as links or credits.
    pub fn description_footer(&self) -> &str {
        &self.description_footer
    }

    pub fn visibility(&self) -> Visibility {
        self.visibility
    }
}

/// The user-editable part of an account, always valid for its network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkAccountDetails {
    handle: String,
    metadata: MetadataDefaults,
    overrides: PresetOverrides,
}

impl NetworkAccountDetails {
    /// Limits are in characters, not bytes. 30 fits the longest handle any
    /// of the five networks allows.
    pub const MAX_HANDLE_CHARS: usize = 30;
    pub const MAX_TAGS: usize = 30;
    pub const MAX_TAG_CHARS: usize = 100;
    pub const MAX_DESCRIPTION_FOOTER_CHARS: usize = 1000;

    /// Validates and normalizes a draft for `network`. The handle loses a
    /// leading `@`; tags lose leading `#`, blank ones are dropped and
    /// repeated ones (ignoring case) are kept once, in their first position.
    pub fn validate(
        network: Network,
        draft: NetworkAccountDraft,
    ) -> Result<Self, Vec<NetworkAccountFieldError>> {
        let mut errors = Vec::new();

        let handle = draft.handle.trim();
        let handle = handle.strip_prefix('@').unwrap_or(handle).to_owned();
        if handle.is_empty() {
            errors.push(NetworkAccountFieldError::HandleRequired);
        } else if handle
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '@' | '/' | '\\' | '?' | '#'))
        {
            errors.push(NetworkAccountFieldError::HandleInvalid);
        } else if too_long(&handle, Self::MAX_HANDLE_CHARS) {
            errors.push(NetworkAccountFieldError::HandleTooLong);
        }

        let tags = normalize_tags(draft.tags);
        if tags.len() > Self::MAX_TAGS {
            errors.push(NetworkAccountFieldError::TooManyTags);
        }
        if tags.iter().any(|tag| too_long(tag, Self::MAX_TAG_CHARS)) {
            errors.push(NetworkAccountFieldError::TagTooLong);
        }

        let description_footer = draft.description_footer.trim().to_owned();
        if too_long(&description_footer, Self::MAX_DESCRIPTION_FOOTER_CHARS) {
            errors.push(NetworkAccountFieldError::DescriptionFooterTooLong);
        }

        if !network.visibilities().contains(&draft.visibility) {
            errors.push(NetworkAccountFieldError::VisibilityNotOffered);
        }

        let bitrate = optional(&draft.bitrate, NetworkAccountFieldError::BitrateInvalid);
        let max_duration = optional(
            &draft.max_duration,
            NetworkAccountFieldError::MaxDurationInvalid,
        );
        let loudness = optional(&draft.loudness, NetworkAccountFieldError::LoudnessInvalid);
        let (bitrate, max_duration, loudness) = match (bitrate, max_duration, loudness) {
            (Ok(bitrate), Ok(max_duration), Ok(loudness)) => (bitrate, max_duration, loudness),
            (bitrate, max_duration, loudness) => {
                errors.extend(bitrate.err());
                errors.extend(max_duration.err());
                errors.extend(loudness.err());
                (None, None, None)
            }
        };

        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(Self {
            handle,
            metadata: MetadataDefaults {
                language: draft.language,
                tags,
                description_footer,
                visibility: draft.visibility,
            },
            overrides: PresetOverrides {
                aspect: draft.aspect,
                resolution: draft.resolution,
                codec: draft.codec,
                bitrate,
                max_duration,
                loudness,
            },
        })
    }

    /// Without the leading `@`.
    pub fn handle(&self) -> &str {
        &self.handle
    }

    pub fn metadata(&self) -> &MetadataDefaults {
        &self.metadata
    }

    pub fn overrides(&self) -> &PresetOverrides {
        &self.overrides
    }
}

impl From<&NetworkAccountDetails> for NetworkAccountDraft {
    fn from(details: &NetworkAccountDetails) -> Self {
        let text = |value: Option<String>| value.unwrap_or_default();
        let overrides = &details.overrides;
        Self {
            handle: details.handle.clone(),
            language: details.metadata.language,
            tags: details.metadata.tags.clone(),
            description_footer: details.metadata.description_footer.clone(),
            visibility: details.metadata.visibility,
            aspect: overrides.aspect,
            resolution: overrides.resolution,
            codec: overrides.codec,
            bitrate: text(overrides.bitrate.map(|v| v.to_string())),
            max_duration: text(overrides.max_duration.map(|v| v.to_string())),
            loudness: text(overrides.loudness.map(|v| v.to_string())),
        }
    }
}

/// Blank text keeps the built-in value; anything else must parse.
fn optional<T: std::str::FromStr>(
    text: &str,
    error: NetworkAccountFieldError,
) -> Result<Option<T>, NetworkAccountFieldError> {
    if text.trim().is_empty() {
        Ok(None)
    } else {
        text.parse().map(Some).map_err(|_| error)
    }
}

fn too_long(text: &str, max_chars: usize) -> bool {
    text.chars().count() > max_chars
}

fn normalize_tags(tags: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    tags.into_iter()
        .map(|tag| tag.trim().trim_start_matches('#').trim().to_owned())
        .filter(|tag| !tag.is_empty() && seen.insert(tag.to_lowercase()))
        .collect()
}

/// A channel's profile on one network (CONTEXT.md). A channel has at most
/// one per network, and the network never changes after the account is
/// added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkAccount {
    pub id: NetworkAccountId,
    pub owner: ProfileId,
    pub channel: ChannelId,
    pub network: Network,
    pub details: NetworkAccountDetails,
}

impl NetworkAccount {
    pub fn new(
        owner: ProfileId,
        channel: ChannelId,
        network: Network,
        details: NetworkAccountDetails,
    ) -> Self {
        Self {
            id: NetworkAccountId::new(),
            owner,
            channel,
            network,
            details,
        }
    }

    /// The network's built-in preset with this account's overrides merged
    /// over it: what a render for this account uses.
    pub fn render_preset(&self) -> RenderPreset {
        self.network.render_preset().with(&self.details.overrides)
    }

    /// The language this account's metadata is written in, given the
    /// channel's content language.
    pub fn language(&self, channel_language: ContentLanguage) -> ContentLanguage {
        self.details.metadata.language.unwrap_or(channel_language)
    }
}

/// Persistence port for network accounts.
pub trait NetworkAccountRepository: Send + Sync {
    /// The channel's accounts, in `Network::ALL` order.
    fn list(&self, channel: ChannelId) -> Result<Vec<NetworkAccount>, RepositoryError>;

    fn get(&self, id: NetworkAccountId) -> Result<Option<NetworkAccount>, RepositoryError>;

    /// Inserts or updates the account.
    fn save(&self, account: &NetworkAccount) -> Result<(), RepositoryError>;

    /// Removes the account. Removing one that does not exist is not an
    /// error.
    fn delete(&self, id: NetworkAccountId) -> Result<(), RepositoryError>;
}

impl<T: NetworkAccountRepository + ?Sized> NetworkAccountRepository for Arc<T> {
    fn list(&self, channel: ChannelId) -> Result<Vec<NetworkAccount>, RepositoryError> {
        (**self).list(channel)
    }

    fn get(&self, id: NetworkAccountId) -> Result<Option<NetworkAccount>, RepositoryError> {
        (**self).get(id)
    }

    fn save(&self, account: &NetworkAccount) -> Result<(), RepositoryError> {
        (**self).save(account)
    }

    fn delete(&self, id: NetworkAccountId) -> Result<(), RepositoryError> {
        (**self).delete(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Bitrate, Loudness, MaxDuration};

    fn draft(handle: &str) -> NetworkAccountDraft {
        NetworkAccountDraft {
            handle: handle.into(),
            ..NetworkAccountDraft::default()
        }
    }

    fn errors(network: Network, draft: NetworkAccountDraft) -> Vec<NetworkAccountFieldError> {
        NetworkAccountDetails::validate(network, draft).unwrap_err()
    }

    #[test]
    fn a_handle_is_enough_for_an_account() {
        let details =
            NetworkAccountDetails::validate(Network::YouTube, draft("spacearchives")).unwrap();
        assert_eq!(details.handle(), "spacearchives");
        assert_eq!(details.metadata().language(), None);
        assert!(details.metadata().tags().is_empty());
        assert_eq!(details.metadata().description_footer(), "");
        assert_eq!(details.metadata().visibility(), Visibility::Public);
        assert!(details.overrides().is_empty());
    }

    #[test]
    fn handle_is_required_and_loses_its_at_sign() {
        assert_eq!(
            errors(Network::TikTok, draft("  ")),
            [NetworkAccountFieldError::HandleRequired]
        );
        assert_eq!(
            errors(Network::TikTok, draft(" @ ")),
            [NetworkAccountFieldError::HandleRequired]
        );
        let details =
            NetworkAccountDetails::validate(Network::TikTok, draft(" @space.archives ")).unwrap();
        assert_eq!(details.handle(), "space.archives");
    }

    #[test]
    fn handle_rejects_spaces_links_and_inner_at_signs() {
        for bad in [
            "space archives",
            "youtube.com/@space",
            "@@space",
            "space@archives",
            "space?x=1",
        ] {
            assert_eq!(
                errors(Network::YouTube, draft(bad)),
                [NetworkAccountFieldError::HandleInvalid],
                "{bad}"
            );
        }
        assert!(
            NetworkAccountDetails::validate(Network::YouTube, draft("Arquivos_do-Espaço")).is_ok()
        );
    }

    #[test]
    fn handle_length_counts_characters() {
        let at_limit = "é".repeat(NetworkAccountDetails::MAX_HANDLE_CHARS);
        assert!(NetworkAccountDetails::validate(Network::X, draft(&at_limit)).is_ok());
        let over = format!(
            "@{}",
            "é".repeat(NetworkAccountDetails::MAX_HANDLE_CHARS + 1)
        );
        assert_eq!(
            errors(Network::X, draft(&over)),
            [NetworkAccountFieldError::HandleTooLong]
        );
    }

    #[test]
    fn tags_lose_hashes_and_duplicates() {
        let details = NetworkAccountDetails::validate(
            Network::InstagramReels,
            NetworkAccountDraft {
                tags: vec![
                    " #space ".into(),
                    "".into(),
                    "##Apollo".into(),
                    "SPACE".into(),
                    " # ".into(),
                    "cold war".into(),
                ],
                ..draft("space")
            },
        )
        .unwrap();
        assert_eq!(details.metadata().tags(), ["space", "Apollo", "cold war"]);
    }

    #[test]
    fn tag_count_is_limited_after_deduplication() {
        let distinct = (0..=NetworkAccountDetails::MAX_TAGS)
            .map(|i| format!("tag{i}"))
            .collect();
        assert_eq!(
            errors(
                Network::YouTube,
                NetworkAccountDraft {
                    tags: distinct,
                    ..draft("space")
                }
            ),
            [NetworkAccountFieldError::TooManyTags]
        );
        let repeated = vec!["same".to_owned(); NetworkAccountDetails::MAX_TAGS + 3];
        assert!(
            NetworkAccountDetails::validate(
                Network::YouTube,
                NetworkAccountDraft {
                    tags: repeated,
                    ..draft("space")
                }
            )
            .is_ok()
        );
    }

    #[test]
    fn visibility_must_be_one_the_network_offers() {
        let unlisted = NetworkAccountDraft {
            visibility: Visibility::Unlisted,
            ..draft("space")
        };
        assert!(NetworkAccountDetails::validate(Network::YouTube, unlisted.clone()).is_ok());
        for network in [
            Network::TikTok,
            Network::InstagramReels,
            Network::X,
            Network::Kick,
        ] {
            assert_eq!(
                errors(network, unlisted.clone()),
                [NetworkAccountFieldError::VisibilityNotOffered]
            );
        }
        let private = NetworkAccountDraft {
            visibility: Visibility::Private,
            ..draft("space")
        };
        assert!(NetworkAccountDetails::validate(Network::TikTok, private).is_ok());
    }

    #[test]
    fn blank_preset_values_keep_the_built_in_ones() {
        let details = NetworkAccountDetails::validate(
            Network::YouTube,
            NetworkAccountDraft {
                bitrate: "  ".into(),
                max_duration: "".into(),
                loudness: " ".into(),
                ..draft("space")
            },
        )
        .unwrap();
        assert!(details.overrides().is_empty());
    }

    #[test]
    fn typed_preset_values_become_overrides() {
        let details = NetworkAccountDetails::validate(
            Network::YouTube,
            NetworkAccountDraft {
                aspect: Some(AspectRatio::Landscape),
                resolution: Some(Resolution::Uhd2160),
                codec: Some(VideoCodec::Hevc),
                bitrate: "35".into(),
                max_duration: "20:00".into(),
                loudness: "-16".into(),
                ..draft("space")
            },
        )
        .unwrap();
        let overrides = details.overrides();
        assert_eq!(overrides.aspect, Some(AspectRatio::Landscape));
        assert_eq!(overrides.resolution, Some(Resolution::Uhd2160));
        assert_eq!(overrides.codec, Some(VideoCodec::Hevc));
        assert_eq!(overrides.bitrate.map(Bitrate::kbps), Some(35_000));
        assert_eq!(overrides.max_duration.map(MaxDuration::seconds), Some(1200));
        assert_eq!(overrides.loudness.map(Loudness::tenths), Some(-160));
    }

    #[test]
    fn every_invalid_field_is_reported() {
        let found = errors(
            Network::X,
            NetworkAccountDraft {
                handle: String::new(),
                tags: vec!["x".repeat(NetworkAccountDetails::MAX_TAG_CHARS + 1)],
                description_footer: "x"
                    .repeat(NetworkAccountDetails::MAX_DESCRIPTION_FOOTER_CHARS + 1),
                visibility: Visibility::Private,
                bitrate: "fast".into(),
                max_duration: "1:75".into(),
                loudness: "14".into(),
                ..NetworkAccountDraft::default()
            },
        );
        assert_eq!(
            found,
            [
                NetworkAccountFieldError::HandleRequired,
                NetworkAccountFieldError::TagTooLong,
                NetworkAccountFieldError::DescriptionFooterTooLong,
                NetworkAccountFieldError::VisibilityNotOffered,
                NetworkAccountFieldError::BitrateInvalid,
                NetworkAccountFieldError::MaxDurationInvalid,
                NetworkAccountFieldError::LoudnessInvalid,
            ]
        );
    }

    #[test]
    fn a_valid_account_round_trips_through_a_draft() {
        let details = NetworkAccountDetails::validate(
            Network::YouTube,
            NetworkAccountDraft {
                handle: "arquivosdoespaco".into(),
                language: Some(ContentLanguage::Portuguese),
                tags: vec!["espaço".into(), "Apollo".into()],
                description_footer: "Inscreva-se!\nhttps://example.com".into(),
                visibility: Visibility::Unlisted,
                aspect: Some(AspectRatio::Landscape),
                resolution: Some(Resolution::Qhd1440),
                codec: Some(VideoCodec::Hevc),
                bitrate: "7,5".into(),
                max_duration: "90".into(),
                loudness: "-13.5".into(),
            },
        )
        .unwrap();
        let draft = NetworkAccountDraft::from(&details);
        assert_eq!(draft.bitrate, "7.5");
        assert_eq!(draft.max_duration, "1:30");
        assert_eq!(
            NetworkAccountDetails::validate(Network::YouTube, draft),
            Ok(details)
        );
    }

    #[test]
    fn the_render_preset_merges_the_overrides_over_the_networks() {
        let details = NetworkAccountDetails::validate(
            Network::TikTok,
            NetworkAccountDraft {
                bitrate: "8".into(),
                ..draft("space")
            },
        )
        .unwrap();
        let account =
            NetworkAccount::new(ProfileId::new(), ChannelId::new(), Network::TikTok, details);
        let preset = account.render_preset();
        assert_eq!(preset.bitrate.kbps(), 8_000);
        assert_eq!(
            RenderPreset {
                bitrate: preset.bitrate,
                ..Network::TikTok.render_preset()
            },
            preset
        );
    }

    #[test]
    fn a_draft_previews_its_preset_ignoring_values_that_do_not_parse_yet() {
        let draft = NetworkAccountDraft {
            codec: Some(VideoCodec::Hevc),
            bitrate: "20".into(),
            max_duration: "1:".into(),
            loudness: "-".into(),
            ..NetworkAccountDraft::default()
        };
        assert_eq!(
            draft.render_preset(Network::X),
            RenderPreset {
                codec: VideoCodec::Hevc,
                bitrate: "20".parse().unwrap(),
                ..Network::X.render_preset()
            }
        );
    }

    #[test]
    fn language_follows_the_channel_unless_set() {
        let account = |language| {
            let details = NetworkAccountDetails::validate(
                Network::X,
                NetworkAccountDraft {
                    language,
                    ..draft("space")
                },
            )
            .unwrap();
            NetworkAccount::new(ProfileId::new(), ChannelId::new(), Network::X, details)
        };
        assert_eq!(
            account(None).language(ContentLanguage::Portuguese),
            ContentLanguage::Portuguese
        );
        assert_eq!(
            account(Some(ContentLanguage::English)).language(ContentLanguage::Portuguese),
            ContentLanguage::English
        );
    }
}
