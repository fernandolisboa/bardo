//! Loose videos (issue #341, ADR-0047): videos with no project and no
//! channel, for a quick try. A loose video holds itself everything a
//! channel would give it, and keeps it as its own settings when it moves
//! into a project.

use std::time::SystemTime;

use crate::{
    ChannelDetails, ChannelDraft, ContentLanguage, Country, Niche, PersonaId, ProfileId, ThemeIdea,
    VideoProject,
};

/// The folder that holds the loose videos' folders, inside the global
/// folder.
pub const LOOSE_VIDEOS_DIR: &str = "Loose videos";

/// What a loose video holds in place of its channel: every setting a
/// channel gives its videos (language, country, format, style or notes,
/// mode, narrator, captions, transition, motion, models, clip fit,
/// thumbnail look) and the angle of the idea it started from. A video
/// moved into a project keeps them as its own: they win over its new
/// channel's, except the channel's name, niche and themes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoSettings {
    /// The angle the video takes, as typed with its title.
    pub angle: String,
    pub details: ChannelDetails,
}

impl VideoSettings {
    /// The settings a loose video starts with: `source`'s (a channel the
    /// user picked to copy from), or a new channel's defaults; the
    /// narrator only when `keep_narrator` says it is global.
    pub fn starting(
        idea: &ThemeIdea,
        source: Option<&ChannelDetails>,
        keep_narrator: impl Fn(PersonaId) -> bool,
    ) -> Self {
        let mut draft = source.map(ChannelDraft::from).unwrap_or_default();
        // A loose video is no channel: it is named after its idea, and
        // lists none of a channel's themes.
        draft.name = idea.title().to_owned();
        draft.themes.clear();
        draft.default_persona = draft.default_persona.filter(|id| keep_narrator(*id));
        let details = ChannelDetails::validate(draft).unwrap_or_else(|_| {
            ChannelDetails::validate(ChannelDraft {
                name: idea.title().to_owned(),
                ..ChannelDraft::default()
            })
            .expect("an idea's title is a valid name")
        });
        Self {
            angle: idea.angle().to_owned(),
            details,
        }
    }

    /// The niche a loose video's prompts name: its settings' niche, else
    /// its title.
    pub fn niche(&self, idea: &ThemeIdea) -> Niche {
        Niche::new(self.details.niche())
            .or_else(|_| {
                Niche::new(
                    &idea
                        .title()
                        .chars()
                        .take(Niche::MAX_CHARS)
                        .collect::<String>(),
                )
            })
            .expect("an idea's title is never blank")
    }

    /// What a video moved into a project reads: these settings, with the
    /// channel's name, niche and themes.
    pub fn under(&self, channel: &ChannelDetails) -> ChannelDetails {
        let mut draft = ChannelDraft::from(&self.details);
        draft.name = channel.name().to_owned();
        draft.niche = channel.niche().to_owned();
        draft.themes = channel.themes().to_vec();
        ChannelDetails::validate(draft).unwrap_or_else(|_| channel.clone())
    }

    /// Whether the channel speaks to another audience than these settings:
    /// its language and country, each when it differs.
    pub fn differences(&self, channel: &ChannelDetails) -> Vec<SettingDifference> {
        let mut found = Vec::new();
        if self.details.language() != channel.language() {
            found.push(SettingDifference::Language {
                video: self.details.language(),
                channel: channel.language(),
            });
        }
        if self.details.country() != channel.country() {
            found.push(SettingDifference::Country {
                video: self.details.country(),
                channel: channel.country(),
            });
        }
        found
    }
}

/// A setting a loose video keeps that differs from the channel it moves
/// to, shown before the move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingDifference {
    Language {
        video: ContentLanguage,
        channel: ContentLanguage,
    },
    Country {
        video: Country,
        channel: Country,
    },
}

/// A new loose video and its settings, from an idea the user typed.
pub fn loose_video(
    owner: ProfileId,
    idea: &ThemeIdea,
    settings: &VideoSettings,
    now: SystemTime,
) -> VideoProject {
    VideoProject::loose(owner, idea, settings.niche(idea), now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CaptionStyle, ClipFit, ProductionMode, StyleChoice, VideoFormat};

    fn idea() -> ThemeIdea {
        ThemeIdea::new(
            "Why the Moon drifts away",
            "Tidal braking, told with a coin",
        )
        .unwrap()
    }

    fn channel(name: &str) -> ChannelDetails {
        ChannelDetails::validate(ChannelDraft {
            name: name.into(),
            niche: "space history".into(),
            themes: vec!["Apollo".into()],
            aesthetic_notes: "Ink on paper".into(),
            language: ContentLanguage::Portuguese,
            country: Country::Brazil,
            format: VideoFormat::Long,
            default_persona: Some(PersonaId::new()),
            caption_style: CaptionStyle::ALL[1],
            production: ProductionMode::ALL[1],
            style: StyleChoice::Catalog(crate::VisualStyle::VintageFilm),
            clip_fit: ClipFit::Cover,
            ..ChannelDraft::default()
        })
        .unwrap()
    }

    #[test]
    fn a_loose_video_copies_a_channel_s_settings_but_not_its_name_or_themes() {
        let source = channel("Céu Profundo");
        let settings = VideoSettings::starting(&idea(), Some(&source), |_| true);
        let details = &settings.details;
        assert_eq!(details.name(), "Why the Moon drifts away");
        assert!(details.themes().is_empty());
        assert_eq!(details.niche(), "space history");
        assert_eq!(details.language(), ContentLanguage::Portuguese);
        assert_eq!(details.country(), Country::Brazil);
        assert_eq!(details.format(), VideoFormat::Long);
        assert_eq!(details.aesthetic_notes(), "Ink on paper");
        assert_eq!(details.caption_style(), source.caption_style());
        assert_eq!(details.production(), source.production());
        assert_eq!(
            details.style(),
            StyleChoice::Catalog(crate::VisualStyle::VintageFilm)
        );
        assert_eq!(details.clip_fit(), ClipFit::Cover);
        assert_eq!(details.default_persona(), source.default_persona());
        assert_eq!(settings.angle, "Tidal braking, told with a coin");
    }

    #[test]
    fn a_project_s_own_narrator_is_left_behind() {
        let source = channel("Céu Profundo");
        let settings = VideoSettings::starting(&idea(), Some(&source), |_| false);
        assert_eq!(settings.details.default_persona(), None);
    }

    #[test]
    fn without_a_channel_it_takes_a_new_channel_s_defaults() {
        let settings = VideoSettings::starting(&idea(), None, |_| true);
        let fresh = ChannelDetails::validate(ChannelDraft {
            name: "Why the Moon drifts away".into(),
            ..ChannelDraft::default()
        })
        .unwrap();
        assert_eq!(settings.details, fresh);
        // No niche of its own: the prompts name its title.
        assert_eq!(settings.niche(&idea()).label(), "Why the Moon drifts away");
    }

    #[test]
    fn a_loose_video_has_no_channel_and_no_theme() {
        let settings = VideoSettings::starting(&idea(), Some(&channel("A")), |_| true);
        let video = loose_video(ProfileId::new(), &idea(), &settings, SystemTime::now());
        assert!(video.is_loose());
        assert_eq!(video.theme, None);
        assert_eq!(video.title, "Why the Moon drifts away");
        assert_eq!(video.niche.label(), "space history");
    }

    #[test]
    fn moved_settings_stay_its_own_under_the_channel_s_name_niche_and_themes() {
        let settings = VideoSettings::starting(&idea(), Some(&channel("A")), |_| true);
        let target = ChannelDetails::validate(ChannelDraft {
            name: "Deep Sky".into(),
            niche: "astronomy".into(),
            themes: vec!["Mars".into()],
            ..ChannelDraft::default()
        })
        .unwrap();
        let read = settings.under(&target);
        assert_eq!(read.name(), "Deep Sky");
        assert_eq!(read.niche(), "astronomy");
        assert_eq!(read.themes(), ["Mars".to_owned()]);
        assert_eq!(read.language(), ContentLanguage::Portuguese);
        assert_eq!(read.format(), VideoFormat::Long);
        assert_eq!(
            settings.differences(&target),
            [
                SettingDifference::Language {
                    video: ContentLanguage::Portuguese,
                    channel: ContentLanguage::English,
                },
                SettingDifference::Country {
                    video: Country::Brazil,
                    channel: Country::UnitedStates,
                },
            ]
        );
        assert!(settings.differences(&settings.details).is_empty());
    }
}
