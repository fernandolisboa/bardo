//! Ad angles (docs/spec/marketing-videos.md, CONTEXT.md): the ways an ad
//! for a product could speak to a channel's audience. Claude proposes
//! them from the product sheet, the decision engine ranks them with typed
//! reasons, and the user edits, discards or approves each one.
//!
//! Approving an angle starts an ad project. The project is a video
//! project like any other, started from a theme whose title and angle are
//! the ad angle's, so scripts, stages, exports and metrics treat it as
//! they treat every video. What a theme does not hold (the product, the
//! framework, the audience segment, the hook, the claims it leans on and
//! the ad ranking) stays in the ad angle beside it.

use std::time::SystemTime;

use crate::{
    AdBrief, AdFramework, ChannelId, CheckedScript, ClaimId, ClaimsCheck, Confidence, JobId, Niche,
    ProductId, ProductSheet, ProfileId, Reason, RepositoryError, Score, Theme, ThemeFieldError,
    ThemeId, ThemeIdea, ThemeStatus, VideoLength, VideoProject, VideoProjectId,
};

uuid_id!(
    /// Identifies an ad angle.
    AdAngleId
);

/// Why typed ad angle text is not valid. One entry per problem, so a form
/// can show them all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AdAngleFieldError {
    TitleRequired,
    TitleTooLong,
    AngleTooLong,
    SegmentTooLong,
    HookRequired,
    HookTooLong,
}

impl AdAngleFieldError {
    pub const ALL: [AdAngleFieldError; 6] = [
        AdAngleFieldError::TitleRequired,
        AdAngleFieldError::TitleTooLong,
        AdAngleFieldError::AngleTooLong,
        AdAngleFieldError::SegmentTooLong,
        AdAngleFieldError::HookRequired,
        AdAngleFieldError::HookTooLong,
    ];
}

impl From<ThemeFieldError> for AdAngleFieldError {
    fn from(error: ThemeFieldError) -> Self {
        match error {
            ThemeFieldError::TitleRequired => AdAngleFieldError::TitleRequired,
            ThemeFieldError::TitleTooLong => AdAngleFieldError::TitleTooLong,
            ThemeFieldError::AngleTooLong => AdAngleFieldError::AngleTooLong,
        }
    }
}

/// The angle itself, always valid: what a theme holds (a working title
/// and the angle), the framework its script follows, the audience segment
/// it speaks to, a draft of its hook and the claims of the product sheet
/// it leans on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdAngleIdea {
    idea: ThemeIdea,
    framework: AdFramework,
    segment: String,
    hook: String,
    claims: Vec<ClaimId>,
}

impl AdAngleIdea {
    /// Limits are in characters, not bytes, so accents count once.
    pub const MAX_SEGMENT_CHARS: usize = 120;
    pub const MAX_HOOK_CHARS: usize = 200;
    /// The most claims an angle leans on; more are left out.
    pub const MAX_CLAIMS: usize = 8;

    /// Validates the text: trimmed, the segment and the hook each on one
    /// line. A claim named twice counts once.
    pub fn new(
        title: &str,
        angle: &str,
        framework: AdFramework,
        segment: &str,
        hook: &str,
        claims: &[ClaimId],
    ) -> Result<Self, Vec<AdAngleFieldError>> {
        let mut errors: Vec<AdAngleFieldError> = Vec::new();
        let idea = match ThemeIdea::new(title, angle) {
            Ok(idea) => Some(idea),
            Err(found) => {
                errors.extend(found.into_iter().map(AdAngleFieldError::from));
                None
            }
        };
        let segment = one_line(segment);
        if segment.chars().count() > Self::MAX_SEGMENT_CHARS {
            errors.push(AdAngleFieldError::SegmentTooLong);
        }
        let hook = one_line(hook);
        if hook.is_empty() {
            errors.push(AdAngleFieldError::HookRequired);
        } else if hook.chars().count() > Self::MAX_HOOK_CHARS {
            errors.push(AdAngleFieldError::HookTooLong);
        }
        let Some(idea) = idea.filter(|_| errors.is_empty()) else {
            return Err(errors);
        };
        let mut distinct: Vec<ClaimId> = Vec::new();
        for claim in claims {
            if !distinct.contains(claim) && distinct.len() < Self::MAX_CLAIMS {
                distinct.push(*claim);
            }
        }
        Ok(Self {
            idea,
            framework,
            segment,
            hook,
            claims: distinct,
        })
    }

    /// The title and angle, as the theme an approved angle starts holds
    /// them.
    pub fn theme_idea(&self) -> &ThemeIdea {
        &self.idea
    }

    pub fn title(&self) -> &str {
        self.idea.title()
    }

    /// What the ad says and why this audience would care; may be empty.
    pub fn angle(&self) -> &str {
        self.idea.angle()
    }

    pub fn framework(&self) -> AdFramework {
        self.framework
    }

    /// The part of the audience it speaks to; may be empty.
    pub fn segment(&self) -> &str {
        &self.segment
    }

    /// A draft of the ad's first words.
    pub fn hook(&self) -> &str {
        &self.hook
    }

    /// The claims of the product sheet it leans on, in the order given.
    pub fn claims(&self) -> &[ClaimId] {
        &self.claims
    }

    /// Whether two angles would read as the same title to the user.
    pub fn same_title(&self, other: &str) -> bool {
        self.idea.same_title(other)
    }
}

/// `text` on one line, trimmed: line breaks and runs of spaces become one
/// space.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What the decision engine said about an ad angle: three typed reasons,
/// each judged for the angle's own text against the product and the
/// channel.
#[derive(Debug, Clone, PartialEq)]
pub struct AdRanking {
    /// Whether the angle speaks to the segment it names. Higher is better.
    pub audience_fit: Reason,
    /// Whether its first two seconds would hold that viewer. Higher is
    /// better.
    pub hook: Reason,
    /// Whether a viewer would know what the product is and what to do
    /// next. Higher is better.
    pub clarity: Reason,
    /// The engine's model, for the record.
    pub model: String,
    pub ranked_at: SystemTime,
}

impl AdRanking {
    /// Composite weights, applied in code rather than asked of the engine
    /// so they stay visible and tunable, like the theme ranking's. The
    /// hook leads: an ad nobody stays for sells nothing.
    /// In hundredths, so that the sum is exact.
    pub const HOOK_WEIGHT: u32 = 40;
    pub const AUDIENCE_FIT_WEIGHT: u32 = 35;
    pub const CLARITY_WEIGHT: u32 = 25;

    /// The ranking key, 0–100: the weighted mean, a half rounded up.
    pub fn priority(&self) -> Score {
        let hundredths = Self::HOOK_WEIGHT * u32::from(self.hook.score.value())
            + Self::AUDIENCE_FIT_WEIGHT * u32::from(self.audience_fit.score.value())
            + Self::CLARITY_WEIGHT * u32::from(self.clarity.score.value());
        // At most 100: the weights add up to 100 and a score is 0 to 100.
        Score::new(((hundredths + 50) / 100) as u8)
    }

    /// The ranking is only as sure as its least sure reason.
    pub fn confidence(&self) -> Confidence {
        [self.audience_fit, self.hook, self.clarity]
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

/// An ad angle action that does not apply to its status (only suggested
/// angles can change).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a {0} ad angle cannot change")]
pub struct AdAngleNotSuggested(pub ThemeStatus);

/// One way an ad for a product could speak to a channel's audience.
#[derive(Debug, Clone, PartialEq)]
pub struct AdAngle {
    pub id: AdAngleId,
    pub owner: ProfileId,
    pub product: ProductId,
    pub channel: ChannelId,
    idea: AdAngleIdea,
    status: ThemeStatus,
    ranking: Option<AdRanking>,
    pub suggested_at: SystemTime,
    /// Its place in the batch it was proposed in, from 0.
    pub position: u32,
    /// The job that proposed it, so a resumed job does not propose twice.
    pub suggestion_job: Option<JobId>,
    /// The theme it became when approved.
    theme: Option<ThemeId>,
}

/// Every stored field of an ad angle, for adapters that rebuild one.
#[derive(Debug, Clone, PartialEq)]
pub struct AdAngleRecord {
    pub id: AdAngleId,
    pub owner: ProfileId,
    pub product: ProductId,
    pub channel: ChannelId,
    pub idea: AdAngleIdea,
    pub status: ThemeStatus,
    pub ranking: Option<AdRanking>,
    pub suggested_at: SystemTime,
    pub position: u32,
    pub suggestion_job: Option<JobId>,
    pub theme: Option<ThemeId>,
}

/// What approving an ad angle starts, saved together or not at all.
#[derive(Debug, Clone, PartialEq)]
pub struct AdStart {
    /// The approved theme that holds the angle's title and text.
    pub theme: Theme,
    /// The ad project, which keeps its product.
    pub project: VideoProject,
}

impl AdAngle {
    /// A freshly proposed, unranked angle.
    #[allow(clippy::too_many_arguments)]
    pub fn suggested(
        owner: ProfileId,
        product: ProductId,
        channel: ChannelId,
        idea: AdAngleIdea,
        suggested_at: SystemTime,
        position: u32,
        suggestion_job: Option<JobId>,
    ) -> Self {
        Self {
            id: AdAngleId::new(),
            owner,
            product,
            channel,
            idea,
            status: ThemeStatus::Suggested,
            ranking: None,
            suggested_at,
            position,
            suggestion_job,
            theme: None,
        }
    }

    pub fn restore(record: AdAngleRecord) -> Self {
        Self {
            id: record.id,
            owner: record.owner,
            product: record.product,
            channel: record.channel,
            idea: record.idea,
            status: record.status,
            ranking: record.ranking,
            suggested_at: record.suggested_at,
            position: record.position,
            suggestion_job: record.suggestion_job,
            theme: record.theme,
        }
    }

    pub fn idea(&self) -> &AdAngleIdea {
        &self.idea
    }

    pub fn status(&self) -> ThemeStatus {
        self.status
    }

    pub fn ranking(&self) -> Option<&AdRanking> {
        self.ranking.as_ref()
    }

    /// The theme an approved angle became.
    pub fn theme(&self) -> Option<ThemeId> {
        self.theme
    }

    /// Whether the angle leans on a claim the sheet marks sensitive.
    /// Bardo says so itself; it is not asked of the engine.
    pub fn leans_on_sensitive(&self, sheet: &ProductSheet) -> bool {
        self.idea
            .claims
            .iter()
            .filter_map(|id| sheet.claim(*id))
            .any(crate::Claim::sensitive)
    }

    fn ensure_suggested(&self) -> Result<(), AdAngleNotSuggested> {
        match self.status {
            ThemeStatus::Suggested => Ok(()),
            status => Err(AdAngleNotSuggested(status)),
        }
    }

    /// Replaces the angle. The old ranking judged the old text, so it is
    /// dropped and the angle waits to be ranked again.
    pub fn edit(&mut self, idea: AdAngleIdea) -> Result<(), AdAngleNotSuggested> {
        self.ensure_suggested()?;
        if idea != self.idea {
            self.idea = idea;
            self.ranking = None;
        }
        Ok(())
    }

    pub fn rank(&mut self, ranking: AdRanking) -> Result<(), AdAngleNotSuggested> {
        self.ensure_suggested()?;
        self.ranking = Some(ranking);
        Ok(())
    }

    pub fn discard(&mut self) -> Result<(), AdAngleNotSuggested> {
        self.ensure_suggested()?;
        self.status = ThemeStatus::Discarded;
        Ok(())
    }

    /// Approves the angle and returns what it starts: a theme with the
    /// angle's title and text in `niche` (the product's category), already
    /// approved, and the ad project of that theme, which keeps the product
    /// and starts at 30 seconds.
    pub fn approve(
        &mut self,
        niche: Niche,
        now: SystemTime,
    ) -> Result<AdStart, AdAngleNotSuggested> {
        self.ensure_suggested()?;
        let mut theme = Theme::suggested(
            self.owner,
            self.channel,
            niche,
            self.idea.idea.clone(),
            now,
            0,
            None,
        );
        let mut project = theme
            .approve(now)
            .expect("a theme just suggested can be approved");
        project.product = Some(self.product);
        project.length = Some(AD_START_LENGTH);
        self.status = ThemeStatus::Approved;
        self.theme = Some(theme.id);
        Ok(AdStart { theme, project })
    }
}

/// How long an ad project runs until the user picks another length.
pub const AD_START_LENGTH: VideoLength = VideoLength::Seconds30;

/// Orders ad angles for the screen: ranked ones by priority, best first;
/// then unranked ones. Ties go to the newer batch, then to the order
/// proposed.
pub fn rank_ad_angles(angles: &mut [AdAngle]) {
    angles.sort_by_key(|angle| {
        (
            std::cmp::Reverse(angle.ranking().map(AdRanking::priority)),
            std::cmp::Reverse(angle.suggested_at),
            angle.position,
        )
    });
}

/// Persistence port for ad angles and the briefs of the ad projects they
/// start. Shared with job worker threads.
pub trait AdRepository: Send + Sync {
    /// Every ad angle of the product for the channel, in any status.
    fn angles(
        &self,
        product: ProductId,
        channel: ChannelId,
    ) -> Result<Vec<AdAngle>, RepositoryError>;

    fn angle(&self, id: AdAngleId) -> Result<Option<AdAngle>, RepositoryError>;

    /// Inserts new angles and updates existing ones, all or none.
    fn save_angles(&self, angles: &[AdAngle]) -> Result<(), RepositoryError>;

    /// Writes the ranking of each of `ranked` and nothing else of it, and
    /// only where the stored angle is still what was ranked: suggested,
    /// without a ranking, with the same words. An angle the user edited,
    /// discarded or approved while it was being ranked keeps what the
    /// user made of it.
    fn save_rankings(&self, ranked: &[AdAngle]) -> Result<(), RepositoryError>;

    /// Saves the approved angle with the theme, the ad project and the
    /// brief it starts, all or none. Refused when the stored angle is no
    /// longer a suggestion, so an angle starts one project at most.
    fn start_ad_project(
        &self,
        angle: &AdAngle,
        start: &AdStart,
        brief: &AdBrief,
    ) -> Result<(), RepositoryError>;

    /// The brief of an ad project; `None` for any other project.
    fn brief(&self, project: VideoProjectId) -> Result<Option<AdBrief>, RepositoryError>;

    /// Saves a brief that exists, with its claims.
    fn save_brief(&self, brief: &AdBrief) -> Result<(), RepositoryError>;

    /// The briefs of every ad project of the product.
    fn briefs_of(&self, product: ProductId) -> Result<Vec<AdBrief>, RepositoryError>;

    /// The claims check of one of the project's scripts, if it was
    /// checked.
    fn claims_check(
        &self,
        project: VideoProjectId,
        of: CheckedScript,
    ) -> Result<Option<ClaimsCheck>, RepositoryError>;

    /// Saves a check in place of the one its script had.
    fn save_claims_check(&self, check: &ClaimsCheck) -> Result<(), RepositoryError>;

    /// Forgets the check of one of the project's scripts.
    fn drop_claims_check(
        &self,
        project: VideoProjectId,
        of: CheckedScript,
    ) -> Result<(), RepositoryError>;

    /// The regenerated script was accepted: its check, if it has one,
    /// becomes the current script's, and the check of the script it
    /// replaced is forgotten either way. All or none.
    fn accept_claims_check(&self, project: VideoProjectId) -> Result<(), RepositoryError>;
}

impl<T: AdRepository + ?Sized> AdRepository for std::sync::Arc<T> {
    fn angles(
        &self,
        product: ProductId,
        channel: ChannelId,
    ) -> Result<Vec<AdAngle>, RepositoryError> {
        (**self).angles(product, channel)
    }

    fn angle(&self, id: AdAngleId) -> Result<Option<AdAngle>, RepositoryError> {
        (**self).angle(id)
    }

    fn save_angles(&self, angles: &[AdAngle]) -> Result<(), RepositoryError> {
        (**self).save_angles(angles)
    }

    fn save_rankings(&self, ranked: &[AdAngle]) -> Result<(), RepositoryError> {
        (**self).save_rankings(ranked)
    }

    fn start_ad_project(
        &self,
        angle: &AdAngle,
        start: &AdStart,
        brief: &AdBrief,
    ) -> Result<(), RepositoryError> {
        (**self).start_ad_project(angle, start, brief)
    }

    fn brief(&self, project: VideoProjectId) -> Result<Option<AdBrief>, RepositoryError> {
        (**self).brief(project)
    }

    fn save_brief(&self, brief: &AdBrief) -> Result<(), RepositoryError> {
        (**self).save_brief(brief)
    }

    fn briefs_of(&self, product: ProductId) -> Result<Vec<AdBrief>, RepositoryError> {
        (**self).briefs_of(product)
    }

    fn claims_check(
        &self,
        project: VideoProjectId,
        of: CheckedScript,
    ) -> Result<Option<ClaimsCheck>, RepositoryError> {
        (**self).claims_check(project, of)
    }

    fn save_claims_check(&self, check: &ClaimsCheck) -> Result<(), RepositoryError> {
        (**self).save_claims_check(check)
    }

    fn drop_claims_check(
        &self,
        project: VideoProjectId,
        of: CheckedScript,
    ) -> Result<(), RepositoryError> {
        (**self).drop_claims_check(project, of)
    }

    fn accept_claims_check(&self, project: VideoProjectId) -> Result<(), RepositoryError> {
        (**self).accept_claims_check(project)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::{Claim, ClaimKind, SheetText};

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn idea(title: &str) -> AdAngleIdea {
        AdAngleIdea::new(
            title,
            "Shows the lamp turning a dark desk into a place to work.",
            AdFramework::BeforeAfter,
            "People who work late at home",
            "Still squinting at your desk at night?",
            &[],
        )
        .unwrap()
    }

    fn angle(title: &str) -> AdAngle {
        AdAngle::suggested(
            ProfileId::new(),
            ProductId::new(),
            ChannelId::new(),
            idea(title),
            at(1_000),
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

    fn ranking(fit: u8, hook: u8, clarity: u8) -> AdRanking {
        AdRanking {
            audience_fit: reason(fit, 0.9),
            hook: reason(hook, 0.8),
            clarity: reason(clarity, 0.7),
            model: "engine-1".into(),
            ranked_at: at(2_000),
        }
    }

    #[test]
    fn an_angle_is_trimmed_and_needs_a_title_and_a_hook() {
        let idea = AdAngleIdea::new(
            "  A desk you can see  ",
            "  The angle.  ",
            AdFramework::Demonstration,
            "  Night\n owls  ",
            "  Lights\n\n on.  ",
            &[],
        )
        .unwrap();
        assert_eq!(idea.title(), "A desk you can see");
        assert_eq!(idea.angle(), "The angle.");
        assert_eq!(idea.segment(), "Night owls");
        assert_eq!(idea.hook(), "Lights on.");
        assert_eq!(idea.framework(), AdFramework::Demonstration);

        assert_eq!(
            AdAngleIdea::new("  ", "", AdFramework::Offer, "", " ", &[]),
            Err(vec![
                AdAngleFieldError::TitleRequired,
                AdAngleFieldError::HookRequired
            ])
        );
    }

    #[test]
    fn angle_limits_count_characters_and_report_every_problem() {
        let fits = AdAngleIdea::new(
            &"é".repeat(ThemeIdea::MAX_TITLE_CHARS),
            &"é".repeat(ThemeIdea::MAX_ANGLE_CHARS),
            AdFramework::Offer,
            &"é".repeat(AdAngleIdea::MAX_SEGMENT_CHARS),
            &"é".repeat(AdAngleIdea::MAX_HOOK_CHARS),
            &[],
        );
        assert!(fits.is_ok());
        assert_eq!(
            AdAngleIdea::new(
                &"a".repeat(ThemeIdea::MAX_TITLE_CHARS + 1),
                &"a".repeat(ThemeIdea::MAX_ANGLE_CHARS + 1),
                AdFramework::Offer,
                &"a".repeat(AdAngleIdea::MAX_SEGMENT_CHARS + 1),
                &"a".repeat(AdAngleIdea::MAX_HOOK_CHARS + 1),
                &[],
            ),
            Err(vec![
                AdAngleFieldError::TitleTooLong,
                AdAngleFieldError::AngleTooLong,
                AdAngleFieldError::SegmentTooLong,
                AdAngleFieldError::HookTooLong,
            ])
        );
    }

    #[test]
    fn a_claim_named_twice_counts_once_and_extra_claims_are_left_out() {
        let first = ClaimId::new();
        let second = ClaimId::new();
        let idea = AdAngleIdea::new(
            "Title",
            "",
            AdFramework::Offer,
            "",
            "Hook",
            &[first, second, first],
        )
        .unwrap();
        assert_eq!(idea.claims(), [first, second]);

        let many: Vec<ClaimId> = (0..AdAngleIdea::MAX_CLAIMS + 3)
            .map(|_| ClaimId::new())
            .collect();
        let idea = AdAngleIdea::new("Title", "", AdFramework::Offer, "", "Hook", &many).unwrap();
        assert_eq!(idea.claims(), &many[..AdAngleIdea::MAX_CLAIMS]);
    }

    #[test]
    fn priority_weighs_the_hook_then_the_audience_then_clarity() {
        assert_eq!(
            AdRanking::HOOK_WEIGHT + AdRanking::AUDIENCE_FIT_WEIGHT + AdRanking::CLARITY_WEIGHT,
            100
        );
        // 0.35 * 80 + 0.40 * 60 + 0.25 * 40 = 62.
        assert_eq!(ranking(80, 60, 40).priority(), Score::new(62));
        // 0.35 * 90 = 31.5, a half that rounds up whatever a float makes
        // of it.
        assert_eq!(ranking(90, 0, 0).priority(), Score::new(32));
        assert_eq!(ranking(100, 100, 100).priority(), Score::MAX);
        assert_eq!(ranking(0, 0, 0).priority(), Score::new(0));
        // The hook weighs most: the same points count for more there.
        assert!(ranking(50, 90, 50).priority() > ranking(90, 50, 50).priority());
        assert!(ranking(90, 50, 50).priority() > ranking(50, 50, 90).priority());
    }

    #[test]
    fn ranking_confidence_is_the_least_sure_reason() {
        assert_eq!(ranking(80, 60, 40).confidence(), Confidence::new(0.7));
    }

    #[test]
    fn editing_changes_the_angle_and_drops_the_old_ranking() {
        let mut angle = angle("A desk you can see");
        angle.rank(ranking(80, 60, 40)).unwrap();

        // The same text again is no edit.
        angle.edit(idea("A desk you can see")).unwrap();
        assert!(angle.ranking().is_some());

        angle.edit(idea("Work after dark")).unwrap();
        assert_eq!(angle.idea().title(), "Work after dark");
        assert_eq!(angle.ranking(), None);
    }

    #[test]
    fn a_change_of_framework_or_claims_alone_is_an_edit() {
        let mut angle = angle("A desk you can see");
        angle.rank(ranking(80, 60, 40)).unwrap();
        let claim = ClaimId::new();
        let leaning = AdAngleIdea::new(
            angle.idea().title(),
            angle.idea().angle(),
            angle.idea().framework(),
            angle.idea().segment(),
            angle.idea().hook(),
            &[claim],
        )
        .unwrap();
        angle.edit(leaning).unwrap();
        assert_eq!(angle.idea().claims(), [claim]);
        assert_eq!(angle.ranking(), None);
    }

    #[test]
    fn approving_starts_a_theme_and_an_ad_project_that_keeps_the_product() {
        let mut angle = angle("A desk you can see");
        let niche = Niche::new("Desk lamps").unwrap();
        let start = angle.approve(niche.clone(), at(5_000)).unwrap();

        assert_eq!(angle.status(), ThemeStatus::Approved);
        assert_eq!(angle.theme(), Some(start.theme.id));

        assert_eq!(start.theme.status(), ThemeStatus::Approved);
        assert_eq!(start.theme.idea(), angle.idea().theme_idea());
        assert_eq!(start.theme.channel, angle.channel);
        assert_eq!(start.theme.niche, niche);

        let project = &start.project;
        assert_eq!(project.theme, start.theme.id);
        assert_eq!(project.channel, angle.channel);
        assert_eq!(project.owner, angle.owner);
        assert_eq!(project.niche, niche);
        assert_eq!(project.title, "A desk you can see");
        assert_eq!(project.product, Some(angle.product));
        assert_eq!(project.length, Some(VideoLength::Seconds30));
        assert_eq!(project.created_at, at(5_000));
    }

    #[test]
    fn only_suggested_angles_change() {
        let mut discarded = angle("One");
        discarded.discard().unwrap();
        assert_eq!(
            discarded.edit(idea("Two")),
            Err(AdAngleNotSuggested(ThemeStatus::Discarded))
        );
        assert!(discarded.rank(ranking(1, 2, 3)).is_err());
        assert!(discarded.discard().is_err());
        assert!(
            discarded
                .approve(Niche::new("Lamps").unwrap(), at(1))
                .is_err()
        );

        let mut approved = angle("Three");
        approved
            .approve(Niche::new("Lamps").unwrap(), at(1))
            .unwrap();
        assert_eq!(
            approved.discard(),
            Err(AdAngleNotSuggested(ThemeStatus::Approved))
        );
        assert!(
            approved
                .approve(Niche::new("Lamps").unwrap(), at(2))
                .is_err()
        );
    }

    #[test]
    fn angles_rank_by_priority_with_unranked_last() {
        let mut low = angle("Low");
        low.rank(ranking(10, 10, 10)).unwrap();
        let mut high = angle("High");
        high.rank(ranking(90, 90, 90)).unwrap();
        let mut second = angle("Unranked second");
        second.position = 1;
        let first = angle("Unranked first");
        let mut newer = angle("Unranked newer");
        newer.suggested_at = at(9_000);

        let mut angles = vec![second, low, first, newer, high];
        rank_ad_angles(&mut angles);
        let titles: Vec<&str> = angles.iter().map(|angle| angle.idea().title()).collect();
        assert_eq!(
            titles,
            [
                "High",
                "Low",
                "Unranked newer",
                "Unranked first",
                "Unranked second"
            ]
        );
    }

    #[test]
    fn an_angle_that_leans_on_a_sensitive_claim_is_marked_by_bardo() {
        let product = ProductId::new();
        let mut sheet = ProductSheet::empty(product, at(1));
        let plain = sheet
            .add_claim("Folds flat", ClaimKind::Feature, at(1))
            .unwrap();
        let numbered = sheet
            .add_claim("Lasts 40 hours on a charge", ClaimKind::Proof, at(1))
            .unwrap();
        assert!(!sheet.claim(plain).is_some_and(Claim::sensitive));
        assert!(sheet.text() == &SheetText::default());

        let leaning = |claims: &[ClaimId]| {
            let idea =
                AdAngleIdea::new("Title", "", AdFramework::Offer, "", "Hook", claims).unwrap();
            AdAngle::suggested(
                ProfileId::new(),
                product,
                ChannelId::new(),
                idea,
                at(1),
                0,
                None,
            )
        };
        assert!(!leaning(&[]).leans_on_sensitive(&sheet));
        assert!(!leaning(&[plain]).leans_on_sensitive(&sheet));
        assert!(leaning(&[plain, numbered]).leans_on_sensitive(&sheet));
        // A claim the sheet no longer holds marks nothing.
        assert!(!leaning(&[ClaimId::new()]).leans_on_sensitive(&sheet));
    }
}
