//! Per-network metadata and the export package (PRD stories 76-78): what a
//! post says on each network (title, description or caption, tags) and the
//! ready-to-post folder the user takes to it by hand.
//!
//! Claude writes each network's metadata from the script and the account's
//! defaults; the user edits it. Each network takes text its own way, so the
//! rules of each one ([`MetadataRules`]) are domain rules: an export holds
//! only metadata that fits them. Limits count characters (Unicode scalar
//! values), as the networks' upload forms count them.

use std::sync::Arc;
use std::time::SystemTime;

use crate::{
    Generation, Network, NetworkAccountId, ProfileId, ProjectFileError, RenderId, RepositoryError,
    VideoProjectId, Visibility,
};

/// Where a network takes a post's tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagPlacement {
    /// A tag field of their own, apart from the text.
    Field,
    /// Hashtags at the end of the caption, counted in its length.
    Hashtags,
}

/// How a network takes a post's text, from its published upload rules
/// (checked 2026-10-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetadataRules {
    /// The title's limit; `None` when the network has no title and the
    /// caption is the post.
    pub title: Option<usize>,
    /// The description's or caption's limit, footer and hashtags included;
    /// `None` when the network takes no text with a video.
    pub text: Option<usize>,
    pub tags: TagPlacement,
    /// How many tags the network takes, when it says.
    pub max_tags: Option<usize>,
    /// How long the tag field may be in all, as the network counts it.
    pub max_tag_chars: Option<usize>,
    /// The network refuses `<` and `>` in the title and description.
    pub no_angle_brackets: bool,
}

impl Network {
    /// - YouTube: title up to 100 characters, description up to 5,000, a
    ///   tag field up to 500 characters in all (a tag with a space counts
    ///   its quotes, and commas between tags count); no `<` or `>`.
    /// - TikTok: no title; the caption, hashtags included, up to 2,200.
    /// - Instagram Reels: no title; the caption up to 2,200, with up to 5
    ///   hashtags (Instagram's cap since December 2025).
    /// - X: no title; the post's text up to 280 (accounts without a paid
    ///   plan), hashtags included.
    /// - Kick: a title and up to 10 tags (its channel tags); no text. Kick
    ///   documents no title limit, so 100 keeps it in line with YouTube's.
    pub fn metadata_rules(self) -> MetadataRules {
        let caption = |limit: usize, max_tags: Option<usize>| MetadataRules {
            title: None,
            text: Some(limit),
            tags: TagPlacement::Hashtags,
            max_tags,
            max_tag_chars: None,
            no_angle_brackets: false,
        };
        match self {
            Network::YouTube => MetadataRules {
                title: Some(100),
                text: Some(5_000),
                tags: TagPlacement::Field,
                max_tags: None,
                max_tag_chars: Some(500),
                no_angle_brackets: true,
            },
            Network::TikTok => caption(2_200, None),
            Network::InstagramReels => caption(2_200, Some(5)),
            Network::X => caption(280, None),
            Network::Kick => MetadataRules {
                title: Some(100),
                text: None,
                tags: TagPlacement::Field,
                max_tags: Some(10),
                max_tag_chars: None,
                no_angle_brackets: false,
            },
        }
    }

    /// How the network lets a post say it holds realistic synthetic
    /// content.
    ///
    /// - YouTube: "Altered or synthetic content" in the upload's details.
    /// - TikTok: the "AI-generated content" switch in the post's settings.
    /// - Instagram: the "AI info" label in the Reel's advanced settings.
    /// - X and Kick: no label of their own, so the post's text says it.
    pub fn disclosure(self) -> DisclosureLabel {
        match self {
            Network::YouTube | Network::TikTok | Network::InstagramReels => {
                DisclosureLabel::Setting
            }
            Network::X | Network::Kick => DisclosureLabel::InText,
        }
    }

    /// The network's name as it brands itself, the same in every language:
    /// the name of its folder in an export.
    pub fn brand(self) -> &'static str {
        match self {
            Network::YouTube => "YouTube",
            Network::TikTok => "TikTok",
            Network::InstagramReels => "Instagram Reels",
            Network::X => "X",
            Network::Kick => "Kick",
        }
    }
}

/// How a network takes a synthetic-content disclosure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisclosureLabel {
    /// A switch or label in the upload form.
    Setting,
    /// No switch: the post's own text says it.
    InText,
}

/// Raw metadata fields as the user typed them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VideoMetadataDraft {
    pub title: String,
    /// The description, or the caption on networks without a title.
    pub description: String,
    /// Without `#`.
    pub tags: Vec<String>,
}

/// What breaks a network's rules. The limit comes from the network's
/// [`MetadataRules`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetadataProblem {
    TitleRequired,
    TitleTooLong,
    /// A network without a title needs a caption.
    TextRequired,
    /// The text with the account's footer and the hashtags is too long.
    TextTooLong,
    TooManyTags,
    /// The tag field is too long in all.
    TagsTooLong,
    /// `<` or `>` where the network refuses them.
    AngleBrackets,
}

impl MetadataProblem {
    pub const ALL: [MetadataProblem; 7] = [
        MetadataProblem::TitleRequired,
        MetadataProblem::TitleTooLong,
        MetadataProblem::TextRequired,
        MetadataProblem::TextTooLong,
        MetadataProblem::TooManyTags,
        MetadataProblem::TagsTooLong,
        MetadataProblem::AngleBrackets,
    ];
}

/// A post as the network takes it: what the user pastes into each field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Post {
    pub network: Network,
    /// `None` on networks without a title.
    pub title: Option<String>,
    /// The description or caption with the footer and, where the network
    /// puts them there, the hashtags; `None` on networks without text.
    pub text: Option<String>,
    /// The tag field's tags; empty on networks that take hashtags.
    pub tags: Vec<String>,
    pub visibility: Visibility,
}

impl Post {
    /// The tag field as YouTube counts it: tags joined by commas, a tag
    /// with a space in quotes.
    pub fn tag_field(&self) -> String {
        self.tags
            .iter()
            .map(|tag| {
                if tag.contains(char::is_whitespace) {
                    format!("\"{tag}\"")
                } else {
                    tag.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    /// A stable fingerprint of the post (FNV-1a over its fields), so an
    /// export can tell whether the metadata changed since.
    pub fn fingerprint(&self) -> String {
        let fields = [
            self.network.code(),
            self.title.as_deref().unwrap_or(""),
            self.text.as_deref().unwrap_or(""),
            &self.tag_field(),
            self.visibility.code(),
        ];
        let hash = fields
            .iter()
            .flat_map(|field| field.bytes().chain(std::iter::once(0)))
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
            });
        format!("{hash:016x}")
    }
}

fn chars(text: &str) -> usize {
    text.chars().count()
}

/// Tags as a network keeps them: without leading `#`, trimmed, blank ones
/// dropped, repeats (ignoring case) kept once in their first place. A
/// hashtag cannot hold spaces, so on networks that take hashtags the words
/// run together.
pub fn normalize_tags(network: Network, tags: &[String]) -> Vec<String> {
    let hashtags = network.metadata_rules().tags == TagPlacement::Hashtags;
    let mut seen = std::collections::HashSet::new();
    tags.iter()
        .map(|tag| {
            let tag = tag.trim().trim_start_matches('#').trim();
            if hashtags {
                tag.split_whitespace().collect::<String>()
            } else {
                tag.split_whitespace().collect::<Vec<_>>().join(" ")
            }
        })
        .filter(|tag| !tag.is_empty() && seen.insert(tag.to_lowercase()))
        .collect()
}

/// One network's metadata for a video project: as Claude wrote it or as
/// the user edited it, and the generation it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoMetadata {
    pub project: VideoProjectId,
    pub owner: ProfileId,
    pub network: Network,
    title: String,
    description: String,
    tags: Vec<String>,
    /// The call that wrote it; one call writes every network's.
    generation: Generation,
    edited: bool,
    pub updated_at: SystemTime,
}

impl VideoMetadata {
    /// What `generation` wrote for `network`, normalized.
    pub fn generated(
        network: Network,
        draft: VideoMetadataDraft,
        generation: Generation,
        now: SystemTime,
    ) -> Self {
        let (title, description, tags) = Self::normalized(network, draft);
        Self {
            project: generation.project,
            owner: generation.owner,
            network,
            title,
            description,
            tags,
            generation,
            edited: false,
            updated_at: now,
        }
    }

    /// Stored metadata as it was saved.
    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        project: VideoProjectId,
        owner: ProfileId,
        network: Network,
        draft: VideoMetadataDraft,
        generation: Generation,
        edited: bool,
        updated_at: SystemTime,
    ) -> Self {
        Self {
            project,
            owner,
            network,
            title: draft.title,
            description: draft.description,
            tags: draft.tags,
            generation,
            edited,
            updated_at,
        }
    }

    fn normalized(network: Network, draft: VideoMetadataDraft) -> (String, String, Vec<String>) {
        let rules = network.metadata_rules();
        let title = match rules.title {
            Some(_) => draft.title.split_whitespace().collect::<Vec<_>>().join(" "),
            None => String::new(),
        };
        let description = match rules.text {
            Some(_) => draft.description.trim().to_owned(),
            None => String::new(),
        };
        (title, description, normalize_tags(network, &draft.tags))
    }

    /// Replaces the fields with the user's; `false` when nothing changed.
    pub fn edit(&mut self, draft: VideoMetadataDraft, now: SystemTime) -> bool {
        let (title, description, tags) = Self::normalized(self.network, draft);
        if (&title, &description, &tags) == (&self.title, &self.description, &self.tags) {
            return false;
        }
        self.title = title;
        self.description = description;
        self.tags = tags;
        self.edited = true;
        self.updated_at = now;
        true
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    pub fn generation(&self) -> &Generation {
        &self.generation
    }

    /// Whether the user changed what Claude wrote.
    pub fn is_edited(&self) -> bool {
        self.edited
    }

    pub fn draft(&self) -> VideoMetadataDraft {
        VideoMetadataDraft {
            title: self.title.clone(),
            description: self.description.clone(),
            tags: self.tags.clone(),
        }
    }

    /// The post as the network takes it, with the account's description
    /// footer and visibility.
    pub fn post(&self, footer: &str, visibility: Visibility) -> Post {
        compose(self.network, &self.draft(), footer, visibility)
    }

    /// What breaks the network's rules, with the account's footer in
    /// place. An export holds only metadata with none.
    pub fn problems(&self, footer: &str) -> Vec<MetadataProblem> {
        problems(self.network, &self.draft(), footer)
    }
}

/// `draft` as `network` takes it: the text with the footer and, where the
/// network puts them there, the hashtags.
pub fn compose(
    network: Network,
    draft: &VideoMetadataDraft,
    footer: &str,
    visibility: Visibility,
) -> Post {
    let rules = network.metadata_rules();
    let tags = normalize_tags(network, &draft.tags);
    let text = rules.text.map(|_| {
        let mut blocks = vec![
            draft.description.trim().to_owned(),
            footer.trim().to_owned(),
        ];
        if rules.tags == TagPlacement::Hashtags && !tags.is_empty() {
            blocks.push(
                tags.iter()
                    .map(|tag| format!("#{tag}"))
                    .collect::<Vec<_>>()
                    .join(" "),
            );
        }
        blocks.retain(|block| !block.is_empty());
        blocks.join("\n\n")
    });
    Post {
        network,
        title: rules
            .title
            .map(|_| draft.title.split_whitespace().collect::<Vec<_>>().join(" ")),
        text,
        tags: match rules.tags {
            TagPlacement::Field => tags,
            TagPlacement::Hashtags => Vec::new(),
        },
        visibility,
    }
}

/// What in `draft` breaks `network`'s rules, with the account's `footer`
/// in place: what the user's counters and an export check.
pub fn problems(
    network: Network,
    draft: &VideoMetadataDraft,
    footer: &str,
) -> Vec<MetadataProblem> {
    let rules = network.metadata_rules();
    let post = compose(network, draft, footer, Visibility::Public);
    let tags = normalize_tags(network, &draft.tags);
    let mut problems = Vec::new();
    if let (Some(limit), Some(title)) = (rules.title, &post.title) {
        if title.is_empty() {
            problems.push(MetadataProblem::TitleRequired);
        } else if chars(title) > limit {
            problems.push(MetadataProblem::TitleTooLong);
        }
    }
    if let (Some(limit), Some(text)) = (rules.text, &post.text) {
        if rules.title.is_none() && draft.description.trim().is_empty() {
            problems.push(MetadataProblem::TextRequired);
        } else if chars(text) > limit {
            problems.push(MetadataProblem::TextTooLong);
        }
    }
    if rules.max_tags.is_some_and(|max| tags.len() > max) {
        problems.push(MetadataProblem::TooManyTags);
    }
    if rules
        .max_tag_chars
        .is_some_and(|max| chars(&post.tag_field()) > max)
    {
        problems.push(MetadataProblem::TagsTooLong);
    }
    let bracketed = |text: &Option<String>| text.as_deref().is_some_and(|t| t.contains(['<', '>']));
    if rules.no_angle_brackets && (bracketed(&post.title) || bracketed(&post.text)) {
        problems.push(MetadataProblem::AngleBrackets);
    }
    problems
}

/// One network's export: the folder written, the video file in it, and
/// what it was made from, so the screen can tell when it is out of date.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export {
    pub project: VideoProjectId,
    pub owner: ProfileId,
    pub network: Network,
    pub account: NetworkAccountId,
    /// The project's package folder, under the export root.
    pub package: String,
    /// The video's file name in the network's folder.
    pub video_file: String,
    /// The render it copied.
    pub render: RenderId,
    /// The post it wrote ([`Post::fingerprint`]).
    pub post: String,
    pub exported_at: SystemTime,
}

/// The file the metadata of each network's export is written to.
pub const METADATA_FILE: &str = "metadata.txt";

/// Characters Windows refuses in a file name.
fn file_safe(text: &str, max_chars: usize) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                ' '
            } else {
                c
            }
        })
        .collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let joined: String = words.join(" ").chars().take(max_chars).collect();
    // Windows drops trailing dots and spaces from names.
    joined.trim_end_matches(['.', ' ']).trim().to_owned()
}

/// Names Windows keeps for devices, whatever the extension.
fn reserved(stem: &str) -> bool {
    let upper = stem.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.len() == 4
            && upper.as_bytes()[3].is_ascii_digit())
}

/// The folder a project's export goes to: its title as a safe name, with
/// the start of its id so two projects of the same title stay apart.
pub fn package_folder(title: &str, project: VideoProjectId) -> String {
    let id = project.to_string();
    let short = &id[..8];
    match file_safe(title, 60) {
        name if name.is_empty() => format!("video ({short})"),
        name => format!("{name} ({short})"),
    }
}

/// The video's file name in an export: the post's title (YouTube fills
/// its title field from the file's name), else the project's.
pub fn video_file_name(title: &str) -> String {
    match file_safe(title, 80) {
        name if name.is_empty() || reserved(&name) => "video.mp4".to_owned(),
        name => format!("{name}.mp4"),
    }
}

/// Where exports are written: `<root>/<package>/<network brand>/<file>`.
/// Names are plain names, never paths. Shared with job worker threads.
pub trait ExportFiles: Send + Sync {
    /// Writes the whole file, replacing one of that name; a reader never
    /// sees it half written.
    fn write(
        &self,
        package: &str,
        network: Network,
        name: &str,
        bytes: &[u8],
    ) -> Result<(), ProjectFileError>;

    /// Copies what `source` reads in as `name`, replacing one of that
    /// name; a reader never sees the copy half written.
    fn copy_from(
        &self,
        package: &str,
        network: Network,
        name: &str,
        source: &mut dyn std::io::Read,
    ) -> Result<(), ProjectFileError>;

    /// Removes the file; a file already gone is not an error.
    fn remove(&self, package: &str, network: Network, name: &str) -> Result<(), ProjectFileError>;

    fn exists(&self, package: &str, network: Network, name: &str) -> bool;

    /// The network's folder in the package, for "Show in folder".
    fn folder(&self, package: &str, network: Network) -> std::path::PathBuf;
}

impl<T: ExportFiles + ?Sized> ExportFiles for Arc<T> {
    fn write(
        &self,
        package: &str,
        network: Network,
        name: &str,
        bytes: &[u8],
    ) -> Result<(), ProjectFileError> {
        (**self).write(package, network, name, bytes)
    }

    fn copy_from(
        &self,
        package: &str,
        network: Network,
        name: &str,
        source: &mut dyn std::io::Read,
    ) -> Result<(), ProjectFileError> {
        (**self).copy_from(package, network, name, source)
    }

    fn remove(&self, package: &str, network: Network, name: &str) -> Result<(), ProjectFileError> {
        (**self).remove(package, network, name)
    }

    fn exists(&self, package: &str, network: Network, name: &str) -> bool {
        (**self).exists(package, network, name)
    }

    fn folder(&self, package: &str, network: Network) -> std::path::PathBuf {
        (**self).folder(package, network)
    }
}

/// Persistence port for per-network metadata and exports. Shared with job
/// worker threads.
pub trait ExportRepository: Send + Sync {
    /// The project's metadata, in `Network::ALL` order.
    fn video_metadata(
        &self,
        project: VideoProjectId,
    ) -> Result<Vec<VideoMetadata>, RepositoryError>;

    /// Saves each network's metadata, replacing what the project had for
    /// it, and their generations when new, in one transaction.
    fn save_video_metadata(&self, metadata: &[VideoMetadata]) -> Result<(), RepositoryError>;

    /// The project's exports, in `Network::ALL` order.
    fn exports(&self, project: VideoProjectId) -> Result<Vec<Export>, RepositoryError>;

    /// Saves an export, replacing the project's earlier one for the same
    /// network.
    fn save_export(&self, export: &Export) -> Result<(), RepositoryError>;
}

impl<T: ExportRepository + ?Sized> ExportRepository for Arc<T> {
    fn video_metadata(
        &self,
        project: VideoProjectId,
    ) -> Result<Vec<VideoMetadata>, RepositoryError> {
        (**self).video_metadata(project)
    }

    fn save_video_metadata(&self, metadata: &[VideoMetadata]) -> Result<(), RepositoryError> {
        (**self).save_video_metadata(metadata)
    }

    fn exports(&self, project: VideoProjectId) -> Result<Vec<Export>, RepositoryError> {
        (**self).exports(project)
    }

    fn save_export(&self, export: &Export) -> Result<(), RepositoryError> {
        (**self).save_export(export)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::tests::generation;

    fn draft(title: &str, description: &str, tags: &[&str]) -> VideoMetadataDraft {
        VideoMetadataDraft {
            title: title.into(),
            description: description.into(),
            tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
        }
    }

    fn metadata(network: Network, draft: VideoMetadataDraft) -> VideoMetadata {
        VideoMetadata::generated(network, draft, generation(), SystemTime::UNIX_EPOCH)
    }

    #[test]
    fn each_network_has_its_published_limits() {
        let youtube = Network::YouTube.metadata_rules();
        assert_eq!(youtube.title, Some(100));
        assert_eq!(youtube.text, Some(5_000));
        assert_eq!(youtube.max_tag_chars, Some(500));
        assert_eq!(youtube.tags, TagPlacement::Field);
        let reels = Network::InstagramReels.metadata_rules();
        assert_eq!((reels.title, reels.text), (None, Some(2_200)));
        assert_eq!(reels.max_tags, Some(5));
        assert_eq!(Network::TikTok.metadata_rules().text, Some(2_200));
        assert_eq!(Network::X.metadata_rules().text, Some(280));
        let kick = Network::Kick.metadata_rules();
        assert_eq!(
            (kick.title, kick.text, kick.max_tags),
            (Some(100), None, Some(10))
        );
    }

    #[test]
    fn titles_count_characters_not_bytes() {
        let accented = "é".repeat(100);
        assert!(problems(Network::YouTube, &draft(&accented, "", &[]), "").is_empty());
        let over = "é".repeat(101);
        assert_eq!(
            problems(Network::YouTube, &draft(&over, "", &[]), ""),
            [MetadataProblem::TitleTooLong]
        );
        assert_eq!(
            problems(Network::Kick, &draft("  ", "", &[]), ""),
            [MetadataProblem::TitleRequired]
        );
    }

    #[test]
    fn a_caption_counts_the_footer_and_the_hashtags() {
        // 270 characters of text, then "\n\n#a #b" makes 277: fits X's 280.
        let text = "a".repeat(270);
        assert!(problems(Network::X, &draft("", &text, &["a", "b"]), "").is_empty());
        // A footer pushes it over.
        assert_eq!(
            problems(Network::X, &draft("", &text, &["a", "b"]), "link"),
            [MetadataProblem::TextTooLong]
        );
        assert_eq!(
            problems(Network::X, &draft("", " ", &["a"]), ""),
            [MetadataProblem::TextRequired]
        );
    }

    #[test]
    fn hashtag_networks_cap_the_tag_count_where_they_say() {
        let six = ["a", "b", "c", "d", "e", "f"];
        assert_eq!(
            problems(Network::InstagramReels, &draft("", "Hi", &six), ""),
            [MetadataProblem::TooManyTags]
        );
        assert!(problems(Network::InstagramReels, &draft("", "Hi", &six[..5]), "").is_empty());
        assert!(problems(Network::TikTok, &draft("", "Hi", &six), "").is_empty());
        let eleven: Vec<String> = (0..11).map(|n| format!("tag{n}")).collect();
        let kick = VideoMetadataDraft {
            title: "Title".into(),
            tags: eleven,
            ..VideoMetadataDraft::default()
        };
        assert_eq!(
            problems(Network::Kick, &kick, ""),
            [MetadataProblem::TooManyTags]
        );
    }

    #[test]
    fn youtube_counts_its_tag_field_with_quotes_and_commas() {
        let post = compose(
            Network::YouTube,
            &draft("T", "", &["space history", "nasa"]),
            "",
            Visibility::Public,
        );
        assert_eq!(post.tag_field(), "\"space history\",nasa");
        // 50 tags of 9 characters with 49 commas: 499 characters.
        let fits: Vec<String> = (0..50).map(|n| format!("tag{n:06}")).collect();
        let fits = VideoMetadataDraft {
            title: "T".into(),
            tags: fits,
            ..VideoMetadataDraft::default()
        };
        assert!(problems(Network::YouTube, &fits, "").is_empty());
        let mut over = fits.clone();
        over.tags.push("x".into());
        assert_eq!(
            problems(Network::YouTube, &over, ""),
            [MetadataProblem::TagsTooLong]
        );
    }

    #[test]
    fn youtube_refuses_angle_brackets() {
        assert_eq!(
            problems(Network::YouTube, &draft("A <b> title", "", &[]), ""),
            [MetadataProblem::AngleBrackets]
        );
        assert!(problems(Network::TikTok, &draft("", "a <3 b", &[]), "").is_empty());
    }

    #[test]
    fn composing_puts_each_field_where_the_network_takes_it() {
        let input = draft(" The  Moon ", "What we found.", &["#space", "moon landing"]);
        let youtube = compose(Network::YouTube, &input, "More: link", Visibility::Unlisted);
        assert_eq!(youtube.title.as_deref(), Some("The Moon"));
        assert_eq!(
            youtube.text.as_deref(),
            Some("What we found.\n\nMore: link")
        );
        assert_eq!(youtube.tags, ["space", "moon landing"]);
        assert_eq!(youtube.visibility, Visibility::Unlisted);

        let tiktok = compose(Network::TikTok, &input, "", Visibility::Public);
        assert_eq!(tiktok.title, None);
        assert_eq!(
            tiktok.text.as_deref(),
            Some("What we found.\n\n#space #moonlanding")
        );
        assert!(tiktok.tags.is_empty());

        let kick = compose(Network::Kick, &input, "More: link", Visibility::Public);
        assert_eq!(kick.title.as_deref(), Some("The Moon"));
        assert_eq!(kick.text, None);
        assert_eq!(kick.tags, ["space", "moon landing"]);
    }

    #[test]
    fn tags_lose_hashes_blanks_and_repeats() {
        let tags: Vec<String> = ["#Moon", " moon ", "", "##NASA", "nasa"]
            .map(String::from)
            .to_vec();
        assert_eq!(normalize_tags(Network::YouTube, &tags), ["Moon", "NASA"]);
    }

    #[test]
    fn edits_normalize_and_mark_the_metadata_edited() {
        let mut meta = metadata(Network::TikTok, draft("ignored", " Caption ", &["a"]));
        assert_eq!(meta.title(), "");
        assert_eq!(meta.description(), "Caption");
        assert!(!meta.is_edited());
        let later = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1);
        assert!(!meta.edit(draft("", "Caption ", &["#a"]), later));
        assert!(!meta.is_edited());
        assert!(meta.edit(draft("", "New caption", &["a", "b"]), later));
        assert!(meta.is_edited());
        assert_eq!(meta.tags(), ["a", "b"]);
        assert_eq!(meta.updated_at, later);
    }

    #[test]
    fn the_fingerprint_follows_every_field() {
        let input = draft("T", "D", &["a"]);
        let post = |input: &VideoMetadataDraft, visibility| {
            compose(Network::YouTube, input, "", visibility).fingerprint()
        };
        let base = post(&input, Visibility::Public);
        assert_eq!(base, post(&input, Visibility::Public));
        assert_ne!(base, post(&input, Visibility::Private));
        assert_ne!(base, post(&draft("T", "D", &["b"]), Visibility::Public));
        assert_ne!(base, post(&draft("T2", "D", &["a"]), Visibility::Public));
    }

    #[test]
    fn package_and_file_names_are_safe_on_windows() {
        let project = VideoProjectId::from(uuid::Uuid::nil());
        assert_eq!(
            package_folder("Who: built <it>? ", project),
            "Who built it (00000000)"
        );
        assert_eq!(package_folder("...", project), "video (00000000)");
        assert_eq!(video_file_name("The Moon / Part 1."), "The Moon Part 1.mp4");
        assert_eq!(video_file_name("con"), "video.mp4");
        assert_eq!(video_file_name("COM1"), "video.mp4");
        assert_eq!(video_file_name("Comet"), "Comet.mp4");
        assert_eq!(video_file_name(""), "video.mp4");
        assert_eq!(video_file_name(&"a".repeat(200)).chars().count(), 84);
    }

    #[test]
    fn three_networks_have_a_disclosure_setting() {
        let settings: Vec<Network> = Network::ALL
            .into_iter()
            .filter(|network| network.disclosure() == DisclosureLabel::Setting)
            .collect();
        assert_eq!(
            settings,
            [Network::YouTube, Network::TikTok, Network::InstagramReels]
        );
    }
}
