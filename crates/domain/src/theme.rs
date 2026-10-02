//! Themes (CONTEXT.md): specific video ideas inside a niche. Claude
//! proposes them, the decision engine ranks them with typed reasons, and
//! the user edits, discards or approves each one. Approving starts a video
//! project (PRD stories 25-28).

use std::fmt;
use std::sync::Arc;
use std::time::SystemTime;

use crate::{ChannelId, Confidence, JobId, Niche, PersonaId, ProfileId, RepositoryError, Score};

uuid_id!(
    /// Identifies a theme.
    ThemeId
);
uuid_id!(
    /// Identifies a video project.
    VideoProjectId
);

/// Why typed theme text is not valid. One entry per problem, so a form can
/// show them all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemeFieldError {
    TitleRequired,
    TitleTooLong,
    AngleTooLong,
}

impl ThemeFieldError {
    pub const ALL: [ThemeFieldError; 3] = [
        ThemeFieldError::TitleRequired,
        ThemeFieldError::TitleTooLong,
        ThemeFieldError::AngleTooLong,
    ];
}

/// The idea itself: a working title and the angle the video takes. Always
/// valid: title present, both trimmed and within limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeIdea {
    title: String,
    angle: String,
}

impl ThemeIdea {
    /// Limits are in characters, not bytes, so accents count once.
    pub const MAX_TITLE_CHARS: usize = 120;
    pub const MAX_ANGLE_CHARS: usize = 500;

    pub fn new(title: &str, angle: &str) -> Result<Self, Vec<ThemeFieldError>> {
        let title = title.trim();
        let angle = angle.trim();
        let mut errors = Vec::new();
        if title.is_empty() {
            errors.push(ThemeFieldError::TitleRequired);
        } else if title.chars().count() > Self::MAX_TITLE_CHARS {
            errors.push(ThemeFieldError::TitleTooLong);
        }
        if angle.chars().count() > Self::MAX_ANGLE_CHARS {
            errors.push(ThemeFieldError::AngleTooLong);
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(Self {
            title: title.to_owned(),
            angle: angle.to_owned(),
        })
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    /// What the video covers and why it hooks; may be empty.
    pub fn angle(&self) -> &str {
        &self.angle
    }

    /// Whether two ideas would read as the same title to the user.
    pub fn same_title(&self, other: &str) -> bool {
        normalized(&self.title) == normalized(other)
    }
}

fn normalized(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Where a theme is in the user's review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemeStatus {
    /// Proposed, waiting for the user.
    Suggested,
    /// The user started a video project from it.
    Approved,
    /// The user set it aside.
    Discarded,
}

impl ThemeStatus {
    pub const ALL: [ThemeStatus; 3] = [
        ThemeStatus::Suggested,
        ThemeStatus::Approved,
        ThemeStatus::Discarded,
    ];

    /// Stable name stored in the database.
    pub fn code(self) -> &'static str {
        match self {
            ThemeStatus::Suggested => "suggested",
            ThemeStatus::Approved => "approved",
            ThemeStatus::Discarded => "discarded",
        }
    }
}

impl fmt::Display for ThemeStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown theme status: {0}")]
pub struct UnknownThemeStatus(pub String);

impl std::str::FromStr for ThemeStatus {
    type Err = UnknownThemeStatus;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ThemeStatus::ALL
            .into_iter()
            .find(|status| status.code() == s)
            .ok_or_else(|| UnknownThemeStatus(s.to_owned()))
    }
}

/// One typed reason behind a ranking: the engine's 0–100 score on one
/// question and how sure it was.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reason {
    pub score: Score,
    pub confidence: Confidence,
}

/// What the decision engine said about a theme. Competition and trend are
/// judged for the theme's own angle, with the niche's research numbers as
/// context; fit is judged against the channel.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeRanking {
    /// How well the idea fits the channel. Higher is better.
    pub fit: Reason,
    /// Viewer demand for the idea right now. Higher is better.
    pub trend: Reason,
    /// How crowded the angle is. Higher is harder.
    pub competition: Reason,
    /// The engine's model, for the record.
    pub model: String,
    pub ranked_at: SystemTime,
}

impl ThemeRanking {
    /// Composite weights, applied in code rather than asked of the engine
    /// so they stay visible and tunable. Fit leads: a hot idea the channel
    /// cannot own is not worth making.
    pub const FIT_WEIGHT: f64 = 0.40;
    pub const TREND_WEIGHT: f64 = 0.35;
    pub const OPEN_WEIGHT: f64 = 0.25;

    /// The ranking key, 0–100.
    pub fn priority(&self) -> Score {
        let open = 100.0 - f64::from(self.competition.score.value());
        let value = Self::FIT_WEIGHT * f64::from(self.fit.score.value())
            + Self::TREND_WEIGHT * f64::from(self.trend.score.value())
            + Self::OPEN_WEIGHT * open;
        Score::new(value.round() as u8)
    }

    /// The ranking is only as sure as its least sure reason.
    pub fn confidence(&self) -> Confidence {
        [self.fit, self.trend, self.competition]
            .into_iter()
            .map(|reason| reason.confidence)
            .fold(
                Confidence::new(1.0),
                |lowest, c| {
                    if c < lowest { c } else { lowest }
                },
            )
    }
}

/// A theme action that does not apply to its status (only suggested themes
/// can change).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a {0} theme cannot change")]
pub struct ThemeNotSuggested(pub ThemeStatus);

/// A video idea for a channel, inside one of its niches.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub id: ThemeId,
    pub owner: ProfileId,
    pub channel: ChannelId,
    pub niche: Niche,
    idea: ThemeIdea,
    status: ThemeStatus,
    ranking: Option<ThemeRanking>,
    pub suggested_at: SystemTime,
    /// Its place in the batch it was proposed in, from 0.
    pub position: u32,
    /// The job that proposed it, so a resumed job does not propose twice.
    pub suggestion_job: Option<JobId>,
}

/// Every stored field of a theme, for adapters that rebuild one.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeRecord {
    pub id: ThemeId,
    pub owner: ProfileId,
    pub channel: ChannelId,
    pub niche: Niche,
    pub idea: ThemeIdea,
    pub status: ThemeStatus,
    pub ranking: Option<ThemeRanking>,
    pub suggested_at: SystemTime,
    pub position: u32,
    pub suggestion_job: Option<JobId>,
}

impl Theme {
    /// A freshly proposed, unranked theme.
    pub fn suggested(
        owner: ProfileId,
        channel: ChannelId,
        niche: Niche,
        idea: ThemeIdea,
        suggested_at: SystemTime,
        position: u32,
        suggestion_job: Option<JobId>,
    ) -> Self {
        Self {
            id: ThemeId::new(),
            owner,
            channel,
            niche,
            idea,
            status: ThemeStatus::Suggested,
            ranking: None,
            suggested_at,
            position,
            suggestion_job,
        }
    }

    pub fn restore(record: ThemeRecord) -> Self {
        Self {
            id: record.id,
            owner: record.owner,
            channel: record.channel,
            niche: record.niche,
            idea: record.idea,
            status: record.status,
            ranking: record.ranking,
            suggested_at: record.suggested_at,
            position: record.position,
            suggestion_job: record.suggestion_job,
        }
    }

    pub fn idea(&self) -> &ThemeIdea {
        &self.idea
    }

    pub fn status(&self) -> ThemeStatus {
        self.status
    }

    pub fn ranking(&self) -> Option<&ThemeRanking> {
        self.ranking.as_ref()
    }

    fn ensure_suggested(&self) -> Result<(), ThemeNotSuggested> {
        match self.status {
            ThemeStatus::Suggested => Ok(()),
            status => Err(ThemeNotSuggested(status)),
        }
    }

    /// Replaces the idea. The old ranking judged the old text, so it is
    /// dropped and the theme waits to be ranked again.
    pub fn edit(&mut self, idea: ThemeIdea) -> Result<(), ThemeNotSuggested> {
        self.ensure_suggested()?;
        if idea != self.idea {
            self.idea = idea;
            self.ranking = None;
        }
        Ok(())
    }

    pub fn rank(&mut self, ranking: ThemeRanking) -> Result<(), ThemeNotSuggested> {
        self.ensure_suggested()?;
        self.ranking = Some(ranking);
        Ok(())
    }

    pub fn discard(&mut self) -> Result<(), ThemeNotSuggested> {
        self.ensure_suggested()?;
        self.status = ThemeStatus::Discarded;
        Ok(())
    }

    /// Approves the theme and returns the video project it starts, linked
    /// to its channel, niche and theme.
    pub fn approve(&mut self, now: SystemTime) -> Result<VideoProject, ThemeNotSuggested> {
        self.ensure_suggested()?;
        self.status = ThemeStatus::Approved;
        Ok(VideoProject {
            id: VideoProjectId::new(),
            owner: self.owner,
            channel: self.channel,
            niche: self.niche.clone(),
            theme: self.id,
            title: self.idea.title.clone(),
            created_at: now,
            persona: None,
        })
    }
}

/// Orders themes for the screen: ranked ones by priority, best first; then
/// unranked ones. Ties go to the newer batch, then to the order proposed.
pub fn rank_themes(themes: &mut [Theme]) {
    themes.sort_by_key(|theme| {
        (
            std::cmp::Reverse(theme.ranking().map(ThemeRanking::priority)),
            std::cmp::Reverse(theme.suggested_at),
            theme.position,
        )
    });
}

/// One video in production (CONTEXT.md). Script, assets, timeline and
/// publications arrive with their own slices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoProject {
    pub id: VideoProjectId,
    pub owner: ProfileId,
    pub channel: ChannelId,
    pub niche: Niche,
    /// The approved theme it started from.
    pub theme: ThemeId,
    /// The working title, copied from the theme when approved.
    pub title: String,
    pub created_at: SystemTime,
    /// The persona that narrates this video instead of the channel's
    /// default; `None` follows the channel.
    pub persona: Option<PersonaId>,
}

impl VideoProject {
    /// Who narrates the video: its own persona, else the channel's default.
    pub fn narrator(&self, channel_default: Option<PersonaId>) -> Option<PersonaId> {
        self.persona.or(channel_default)
    }
}

/// Persistence port for themes and the video projects they start. Shared
/// with job worker threads.
pub trait ThemeRepository: Send + Sync {
    /// Every theme of the channel, in any status.
    fn themes(&self, channel: ChannelId) -> Result<Vec<Theme>, RepositoryError>;

    fn theme(&self, id: ThemeId) -> Result<Option<Theme>, RepositoryError>;

    /// Inserts new themes and updates existing ones, all or none.
    fn save_themes(&self, themes: &[Theme]) -> Result<(), RepositoryError>;

    /// Saves the approved theme and the project it starts, all or none.
    fn start_project(&self, theme: &Theme, project: &VideoProject) -> Result<(), RepositoryError>;

    /// The channel's video projects, newest first.
    fn projects(&self, channel: ChannelId) -> Result<Vec<VideoProject>, RepositoryError>;

    fn project(&self, id: VideoProjectId) -> Result<Option<VideoProject>, RepositoryError>;

    /// Sets or clears (`None`) the persona that narrates the project.
    fn set_project_persona(
        &self,
        id: VideoProjectId,
        persona: Option<PersonaId>,
    ) -> Result<(), RepositoryError>;
}

impl<T: ThemeRepository + ?Sized> ThemeRepository for Arc<T> {
    fn themes(&self, channel: ChannelId) -> Result<Vec<Theme>, RepositoryError> {
        (**self).themes(channel)
    }

    fn theme(&self, id: ThemeId) -> Result<Option<Theme>, RepositoryError> {
        (**self).theme(id)
    }

    fn save_themes(&self, themes: &[Theme]) -> Result<(), RepositoryError> {
        (**self).save_themes(themes)
    }

    fn start_project(&self, theme: &Theme, project: &VideoProject) -> Result<(), RepositoryError> {
        (**self).start_project(theme, project)
    }

    fn projects(&self, channel: ChannelId) -> Result<Vec<VideoProject>, RepositoryError> {
        (**self).projects(channel)
    }

    fn project(&self, id: VideoProjectId) -> Result<Option<VideoProject>, RepositoryError> {
        (**self).project(id)
    }

    fn set_project_persona(
        &self,
        id: VideoProjectId,
        persona: Option<PersonaId>,
    ) -> Result<(), RepositoryError> {
        (**self).set_project_persona(id, persona)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + secs)
    }

    fn idea(title: &str) -> ThemeIdea {
        ThemeIdea::new(title, "An angle.").unwrap()
    }

    fn theme(title: &str) -> Theme {
        Theme::suggested(
            ProfileId::new(),
            ChannelId::new(),
            Niche::new("space history").unwrap(),
            idea(title),
            at(0),
            0,
            None,
        )
    }

    fn reason(score: u8, confidence: f64) -> Reason {
        Reason {
            score: Score::new(score),
            confidence: Confidence::new(confidence),
        }
    }

    fn ranking(fit: u8, trend: u8, competition: u8) -> ThemeRanking {
        ThemeRanking {
            fit: reason(fit, 0.9),
            trend: reason(trend, 0.8),
            competition: reason(competition, 0.7),
            model: "engine-1".into(),
            ranked_at: at(10),
        }
    }

    #[test]
    fn an_idea_is_trimmed_and_needs_a_title() {
        let idea =
            ThemeIdea::new("  Why Apollo 13 almost failed \n", " A tense retelling. ").unwrap();
        assert_eq!(idea.title(), "Why Apollo 13 almost failed");
        assert_eq!(idea.angle(), "A tense retelling.");
        assert!(ThemeIdea::new("Title only", "").is_ok());
        assert_eq!(
            ThemeIdea::new(" ", ""),
            Err(vec![ThemeFieldError::TitleRequired])
        );
    }

    #[test]
    fn idea_limits_count_characters_and_report_every_problem() {
        let title = "é".repeat(ThemeIdea::MAX_TITLE_CHARS);
        let angle = "é".repeat(ThemeIdea::MAX_ANGLE_CHARS);
        assert!(ThemeIdea::new(&title, &angle).is_ok());
        assert_eq!(
            ThemeIdea::new(&format!("{title}é"), &format!("{angle}é")),
            Err(vec![
                ThemeFieldError::TitleTooLong,
                ThemeFieldError::AngleTooLong
            ])
        );
    }

    #[test]
    fn same_title_ignores_case_and_spacing() {
        assert!(idea("The  Lost Cosmonauts").same_title(" the lost cosmonauts"));
        assert!(!idea("The Lost Cosmonauts").same_title("The Lost Cosmonaut"));
    }

    #[test]
    fn status_codes_round_trip() {
        for status in ThemeStatus::ALL {
            assert_eq!(status.code().parse::<ThemeStatus>(), Ok(status));
        }
        assert!("pending".parse::<ThemeStatus>().is_err());
    }

    #[test]
    fn priority_weighs_fit_trend_and_open_space() {
        // 0.4·80 + 0.35·60 + 0.25·(100−40) = 32 + 21 + 15
        assert_eq!(ranking(80, 60, 40).priority(), Score::new(68));
        assert_eq!(ranking(100, 100, 0).priority(), Score::MAX);
        assert_eq!(ranking(0, 0, 100).priority(), Score::new(0));
        assert!(ranking(90, 50, 50).priority() > ranking(50, 90, 50).priority());
    }

    #[test]
    fn ranking_confidence_is_the_least_sure_reason() {
        assert_eq!(ranking(1, 2, 3).confidence(), Confidence::new(0.7));
    }

    #[test]
    fn editing_changes_the_idea_and_drops_the_old_ranking() {
        let mut theme = theme("Old title");
        theme.rank(ranking(80, 60, 40)).unwrap();

        theme.edit(theme.idea().clone()).unwrap();
        assert!(
            theme.ranking().is_some(),
            "unchanged text keeps the ranking"
        );

        theme.edit(idea("New title")).unwrap();
        assert_eq!(theme.idea().title(), "New title");
        assert_eq!(theme.ranking(), None);
        assert_eq!(theme.status(), ThemeStatus::Suggested);
    }

    #[test]
    fn approving_starts_a_project_linked_to_channel_niche_and_theme() {
        let mut theme = theme("The lost cosmonauts");
        let project = theme.approve(at(99)).unwrap();
        assert_eq!(theme.status(), ThemeStatus::Approved);
        assert_eq!(project.channel, theme.channel);
        assert_eq!(project.owner, theme.owner);
        assert_eq!(project.niche, theme.niche);
        assert_eq!(project.theme, theme.id);
        assert_eq!(project.title, "The lost cosmonauts");
        assert_eq!(project.created_at, at(99));
        assert_eq!(project.persona, None, "follows the channel's persona");
    }

    #[test]
    fn a_projects_own_persona_overrides_the_channels_default() {
        let mut project = theme("The lost cosmonauts").approve(at(1)).unwrap();
        let channel_default = PersonaId::new();
        assert_eq!(
            project.narrator(Some(channel_default)),
            Some(channel_default)
        );
        assert_eq!(project.narrator(None), None);

        let own = PersonaId::new();
        project.persona = Some(own);
        assert_eq!(project.narrator(Some(channel_default)), Some(own));
        assert_eq!(project.narrator(None), Some(own));
    }

    #[test]
    fn only_suggested_themes_change() {
        let mut approved = theme("A");
        approved.approve(at(1)).unwrap();
        let mut discarded = theme("B");
        discarded.discard().unwrap();
        assert_eq!(discarded.status(), ThemeStatus::Discarded);

        for theme in [&mut approved, &mut discarded] {
            let status = theme.status();
            assert_eq!(theme.edit(idea("C")), Err(ThemeNotSuggested(status)));
            assert_eq!(theme.discard(), Err(ThemeNotSuggested(status)));
            assert!(theme.approve(at(2)).is_err());
            assert!(theme.rank(ranking(1, 1, 1)).is_err());
        }
    }

    #[test]
    fn themes_rank_by_priority_with_unranked_last() {
        let mut low = theme("low");
        low.rank(ranking(10, 10, 90)).unwrap();
        let mut high = theme("high");
        high.rank(ranking(90, 90, 10)).unwrap();
        let mut unranked_second = theme("unranked second");
        unranked_second.position = 1;
        let unranked_first = theme("unranked first");
        let mut newer = theme("newer tie");
        newer.rank(ranking(10, 10, 90)).unwrap();
        newer.suggested_at = at(5);

        let mut themes = vec![unranked_second, low, unranked_first, high, newer];
        rank_themes(&mut themes);

        let order: Vec<_> = themes.iter().map(|t| t.idea().title()).collect();
        assert_eq!(
            order,
            [
                "high",
                "newer tie",
                "low",
                "unranked first",
                "unranked second"
            ]
        );
    }
}
