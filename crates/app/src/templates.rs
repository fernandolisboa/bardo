//! Template use cases (PRD story 45): read and edit the profile's
//! templates. Saving changed text adds a version; old versions stay
//! readable, and each generation records the version it used.
//!
//! Every profile starts with Bardo's default text as version 1. Templates
//! are sent to providers, so they are written in English whatever the
//! interface language; the video's language is a variable.

use std::time::SystemTime;

use bardo_domain::{
    ProfileId, RepositoryError, TemplateBody, TemplateFieldError, TemplateKind, TemplateRepository,
    TemplateVersion, TemplateVersionId,
};

use crate::{Bardo, Text};

const SCRIPT_INSTRUCTIONS: &str = "\
You write narration scripts for a faceless YouTube channel: a narrator speaks over visuals, with \
no on-camera host. Write in {{language}}; the audience's country is {{country}}.

The script is original: no copying or close paraphrase of existing videos. Be accurate where the \
story touches real events, and keep it safe for advertisers.

Write only what the narrator says, as plain paragraphs: no headings, scene directions, \
timestamps, speaker labels or markdown.";

const SCRIPT_PROMPT: &str = "\
Channel: {{channel_name}}
Channel niche: {{channel_niche}}
Recurring channel themes: {{channel_themes}}
Aesthetic notes: {{aesthetic_notes}}
Narrator: {{persona}}

Video niche: {{niche}}
Video title: {{theme_title}}
Angle: {{theme_angle}}

Write the narration script for this video, {{target_length}}. Hook the viewer in the first two \
sentences, keep the tension through the middle, and end with a payoff that delivers on the title's \
promise.";

const IMAGE_PROMPT_INSTRUCTIONS: &str = "\
You plan the visuals of a faceless YouTube video: a narrator speaks over a sequence of still \
images, one per scene. You split the narration into scenes and write one image prompt per scene \
for an image model.

Scenes follow the narration in order and together cover all of it: a scene starts at a sentence \
and runs until the next scene starts. Change scene when the subject, place or moment changes, \
about every 4 to 8 seconds of narration; a long sentence may hold a scene on its own.

Each prompt stands alone, because the image model sees one prompt at a time and remembers none \
of the others. Describe the subject, setting, period, composition, lighting and mood, and repeat \
the channel's visual style in every prompt so the images look like one video. Frames are \
{{frame_shape}}. Write the prompts in English, whatever the narration's language ({{language}}).

The images are original: no recognizable real people, logos, brands or copyrighted characters, \
and no text, captions or watermarks in the image. Keep them safe for advertisers.";

const IMAGE_PROMPT_PROMPT: &str = "\
Channel: {{channel_name}}
Channel niche: {{channel_niche}}
Visual style (aesthetic notes): {{aesthetic_notes}}

Video niche: {{niche}}
Video title: {{theme_title}}
Angle: {{theme_angle}}

The narration, one sentence per line: its number, when it is spoken, and its text.
{{narration_sentences}}

Split the narration into scenes. For each scene give the number of the sentence it starts at \
and the image prompt that draws it.";

const MUSIC_PROMPT_INSTRUCTIONS: &str = "\
You write prompts for AI music tools (text-to-music generators). The music is the background bed \
of a faceless YouTube video: a narrator speaks over it the whole time, so it supports the voice \
and never competes with it.

Write one prompt of 40 to 80 words in English, as plain text with no title, quotes, lists or \
markdown. Describe genre, mood, tempo in BPM, key instruments and how the energy moves over the \
track. The music is instrumental: no vocals, no choirs singing words, no spoken parts. Leave room \
in the midrange where the voice sits.

The music is original: never name artists, bands, songs, composers or soundtracks, and never ask \
for an imitation of one. Keep it safe for advertisers.";

const MUSIC_PROMPT_PROMPT: &str = "\
Channel: {{channel_name}}
Channel niche: {{channel_niche}}
Aesthetic notes: {{aesthetic_notes}}
Audience country: {{country}}

Video niche: {{niche}}
Video title: {{theme_title}}
Angle: {{theme_angle}}
Video length: {{video_length}}

Write the music prompt for this video. The track should loop or run for the video's length \
without a hard ending.";

const METADATA_INSTRUCTIONS: &str = "\
You write the post text for a faceless video that will be posted by hand on several social \
networks. For each network you are given, write what its upload form asks for, within its \
limits: a title where it has one, a description or caption where it takes one, and tags.

Write each network's text in the language given for it. Lead with a hook that makes people want \
to watch, and keep it true to the script: no claims the video does not make, no clickbait it \
does not pay off. Keep it safe for advertisers.

Tags are single topics without the # sign. Start from the account's default tags when it has \
some, then add the most specific topics of this video, and stop within the network's tag limits. \
Leave links, credits and calls to follow out of the text: the account adds its own footer.

Stay well inside every limit: the footer and hashtags are added to the text after you write it.";

const METADATA_PROMPT: &str = "\
Channel: {{channel_name}}
Channel niche: {{channel_niche}}
Channel language: {{language}}
Audience country: {{country}}

Video niche: {{niche}}
Video title: {{theme_title}}
Angle: {{theme_angle}}

The networks, one per line, with what each one takes:
{{networks}}

The script:
{{script}}

Write the title, description and tags for each network listed. Leave a field empty where the \
network does not take it.";

const THUMBNAIL_INSTRUCTIONS: &str = "\
You write the YouTube thumbnail of a faceless long-form video: a short text that Bardo draws in \
big letters over a background picture, and the prompt an image model draws that background from.

The text is a hook of one to five words, at most 40 characters, in {{language}}. It adds to the \
video's title instead of repeating it: a question, a stake or a contradiction that makes people \
want to know more. No hashtags, emoji or quotes, and no claim the video does not pay off.

The background prompt is in English, whatever the video's language. Describe one strong subject \
with high contrast and a clear, uncluttered side where the text can sit, in the channel's visual \
style, wide (16:9). The picture is original: no recognizable real people, logos, brands or \
copyrighted characters, and no text, letters or watermarks in it. Keep it safe for advertisers.";

const THUMBNAIL_PROMPT: &str = "\
Channel: {{channel_name}}
Channel niche: {{channel_niche}}
Visual style (aesthetic notes): {{aesthetic_notes}}

Video niche: {{niche}}
Video title on YouTube: {{video_title}}
Working title: {{theme_title}}
Angle: {{theme_angle}}

Write the thumbnail's text and the prompt for its background.";

const CHAPTERS_INSTRUCTIONS: &str = "\
You split a faceless long-form YouTube video into chapters, which YouTube shows as named parts \
of the progress bar. You are given the narration, one numbered sentence per line with when it is \
spoken.

Start a chapter where the story moves to a new part: a new question, place, period or turn. The \
first chapter starts at sentence 0. Give between 3 and 12 chapters, each at least 30 seconds \
long, so every chapter is worth jumping to.

Each title names what the part is about in two to six words, in {{language}}, with no numbers, \
timestamps or emoji. Keep titles true to the narration: no spoilers in the first chapters' titles \
and no clickbait.";

const CHAPTERS_PROMPT: &str = "\
Video title: {{theme_title}}
Angle: {{theme_angle}}
Video length: {{video_length}}

The narration, one sentence per line: its number, when it is spoken, and its text.
{{narration_sentences}}

Split the video into chapters. For each chapter give the number of the sentence it starts at and \
its title.";

/// Bardo's earlier default texts, before the video format (#179): one
/// fixed script length and wide frames. A profile whose current version is
/// still one of these, untouched, moves on to today's default.
const RETIRED_SCRIPT_PROMPT: &str = "\
Channel: {{channel_name}}
Channel niche: {{channel_niche}}
Recurring channel themes: {{channel_themes}}
Aesthetic notes: {{aesthetic_notes}}
Narrator: {{persona}}

Video niche: {{niche}}
Video title: {{theme_title}}
Angle: {{theme_angle}}

Write the narration script for this video, about 1,200 to 1,500 words (8 to 10 minutes spoken). \
Hook the viewer in the first two sentences, keep the tension through the middle, and end with a \
payoff that delivers on the title's promise.";

const RETIRED_IMAGE_PROMPT_INSTRUCTIONS: &str = "\
You plan the visuals of a faceless YouTube video: a narrator speaks over a sequence of still \
images, one per scene. You split the narration into scenes and write one image prompt per scene \
for an image model.

Scenes follow the narration in order and together cover all of it: a scene starts at a sentence \
and runs until the next scene starts. Change scene when the subject, place or moment changes, \
about every 4 to 8 seconds of narration; a long sentence may hold a scene on its own.

Each prompt stands alone, because the image model sees one prompt at a time and remembers none \
of the others. Describe the subject, setting, period, composition, lighting and mood, and repeat \
the channel's visual style in every prompt so the images look like one video. Frames are wide \
(16:9). Write the prompts in English, whatever the narration's language ({{language}}).

The images are original: no recognizable real people, logos, brands or copyrighted characters, \
and no text, captions or watermarks in the image. Keep them safe for advertisers.";

/// Bardo's earlier defaults for `kind`, oldest first.
fn retired_defaults(kind: TemplateKind) -> Vec<TemplateBody> {
    let (instructions, prompt) = match kind {
        TemplateKind::Script => (SCRIPT_INSTRUCTIONS, RETIRED_SCRIPT_PROMPT),
        TemplateKind::ImagePrompt => (RETIRED_IMAGE_PROMPT_INSTRUCTIONS, IMAGE_PROMPT_PROMPT),
        TemplateKind::MusicPrompt
        | TemplateKind::Metadata
        | TemplateKind::Thumbnail
        | TemplateKind::Chapters => return Vec::new(),
    };
    vec![TemplateBody::new(kind, instructions, prompt).expect("the retired templates are valid")]
}

/// The current body with each part still at one of Bardo's earlier
/// defaults moved on to today's, or `None` when nothing moves. The
/// instructions and the prompt move apart, so editing one keeps the other's
/// upgrade. A part moves once: when some version already holds today's
/// default for it, an earlier default there is the user's own choice.
fn upgraded(
    kind: TemplateKind,
    current: &TemplateBody,
    versions: &[TemplateVersion],
) -> Option<TemplateBody> {
    let today = default_template(kind);
    let retired = retired_defaults(kind);
    let moves = |part: fn(&TemplateBody) -> &str| {
        retired.iter().any(|old| part(old) == part(current))
            && part(&today) != part(current)
            && versions
                .iter()
                .all(|version| part(&version.body) != part(&today))
    };
    let instructions = moves(TemplateBody::instructions);
    let prompt = moves(TemplateBody::prompt);
    if !instructions && !prompt {
        return None;
    }
    let pick = |moved: bool, part: fn(&TemplateBody) -> &str| {
        if moved { part(&today) } else { part(current) }
    };
    TemplateBody::new(
        kind,
        pick(instructions, TemplateBody::instructions),
        pick(prompt, TemplateBody::prompt),
    )
    .ok()
}

/// Bardo's own text for `kind`: every profile's version 1.
pub fn default_template(kind: TemplateKind) -> TemplateBody {
    let (instructions, prompt) = match kind {
        TemplateKind::Script => (SCRIPT_INSTRUCTIONS, SCRIPT_PROMPT),
        TemplateKind::ImagePrompt => (IMAGE_PROMPT_INSTRUCTIONS, IMAGE_PROMPT_PROMPT),
        TemplateKind::MusicPrompt => (MUSIC_PROMPT_INSTRUCTIONS, MUSIC_PROMPT_PROMPT),
        TemplateKind::Metadata => (METADATA_INSTRUCTIONS, METADATA_PROMPT),
        TemplateKind::Thumbnail => (THUMBNAIL_INSTRUCTIONS, THUMBNAIL_PROMPT),
        TemplateKind::Chapters => (CHAPTERS_INSTRUCTIONS, CHAPTERS_PROMPT),
    };
    TemplateBody::new(kind, instructions, prompt).expect("the default templates are valid")
}

/// Saves version 1 of every template the profile does not have yet.
fn seed_templates(
    templates: &dyn TemplateRepository,
    owner: ProfileId,
) -> Result<(), RepositoryError> {
    for kind in TemplateKind::ALL {
        if templates.template_versions(owner, kind)?.is_empty() {
            let first = TemplateVersion::first(owner, default_template(kind), SystemTime::now());
            templates.add_template_version(&first)?;
        }
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    /// The typed text breaks one or more rules; the editor shows each one.
    #[error("invalid template: {0:?}")]
    Invalid(Vec<TemplateFieldError>),
    #[error("template version not found")]
    NotFound,
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl TemplateError {
    /// What the templates screen says.
    pub fn message(&self) -> Text {
        match self {
            TemplateError::Invalid(errors) => {
                errors.first().map_or(Text::TemplateNotSaved, |error| {
                    Text::TemplateProblem(error.problem)
                })
            }
            TemplateError::NotFound => Text::TemplateNotFound,
            TemplateError::Repository(_) => Text::TemplateNotSaved,
        }
    }

    pub fn field_errors(&self) -> &[TemplateFieldError] {
        match self {
            TemplateError::Invalid(errors) => errors,
            _ => &[],
        }
    }
}

impl Bardo {
    /// Every version of the profile's template of `kind`, newest first. The
    /// first one is what the next generation uses.
    pub fn template_versions(
        &self,
        kind: TemplateKind,
    ) -> Result<Vec<TemplateVersion>, TemplateError> {
        let mut versions = self.templates.template_versions(self.profile.id, kind)?;
        if versions.is_empty() {
            // Version 1 is saved on first use, so profiles from before
            // templates existed get it too.
            seed_templates(&*self.templates, self.profile.id)?;
            versions = self.templates.template_versions(self.profile.id, kind)?;
        }
        let next = versions.last().and_then(|current| {
            current.revise(upgraded(kind, &current.body, &versions)?, SystemTime::now())
        });
        if let Some(next) = next {
            self.templates.add_template_version(&next)?;
            versions.push(next);
        }
        versions.reverse();
        Ok(versions)
    }

    /// The version the next generation of `kind` uses.
    pub fn current_template(&self, kind: TemplateKind) -> Result<TemplateVersion, TemplateError> {
        self.template_versions(kind)?
            .into_iter()
            .next()
            .ok_or(TemplateError::NotFound)
    }

    pub fn template_version(
        &self,
        id: TemplateVersionId,
    ) -> Result<TemplateVersion, TemplateError> {
        self.templates
            .template_version(id)?
            .filter(|version| version.owner == self.profile.id)
            .ok_or(TemplateError::NotFound)
    }

    /// Saves the text as the next version of the template. Unchanged text
    /// adds no version and returns the current one.
    pub fn save_template(
        &self,
        kind: TemplateKind,
        instructions: &str,
        prompt: &str,
    ) -> Result<TemplateVersion, TemplateError> {
        let body = TemplateBody::new(kind, instructions, prompt).map_err(TemplateError::Invalid)?;
        let current = self.current_template(kind)?;
        match current.revise(body, SystemTime::now()) {
            Some(next) => {
                self.templates.add_template_version(&next)?;
                Ok(next)
            }
            None => Ok(current),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bardo_domain::{TemplateField, TemplateProblem};
    use bardo_storage::{Database, MemorySecretStore};

    use super::*;
    use crate::{Repositories, testing};

    fn start(db: &Arc<Database>) -> Bardo {
        let repositories =
            Repositories::shared(Arc::clone(db), Arc::new(MemorySecretStore::default()));
        Bardo::start(repositories, testing::providers(), Some("en-US")).unwrap()
    }

    #[test]
    fn a_new_profile_starts_with_the_default_script_template() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let app = start(&db);
        let versions = app.template_versions(TemplateKind::Script).unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].number, 1);
        assert_eq!(versions[0].body, default_template(TemplateKind::Script));
        assert_eq!(versions[0].owner, app.profile().id);

        drop(app);
        let again = start(&db);
        assert_eq!(
            again.template_versions(TemplateKind::Script).unwrap().len(),
            1,
            "a restart does not seed again"
        );
    }

    #[test]
    fn every_default_template_uses_every_variable_of_its_kind() {
        for kind in TemplateKind::ALL {
            let body = default_template(kind);
            let text = format!("{}\n{}", body.instructions(), body.prompt());
            for variable in kind.variables() {
                assert!(
                    text.contains(&variable.placeholder()),
                    "{kind}: {variable:?}"
                );
            }
        }
    }

    #[test]
    fn a_profile_from_before_image_prompts_gets_the_default_on_first_use() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let app = start(&db);
        let versions = app.template_versions(TemplateKind::ImagePrompt).unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(
            versions[0].body,
            default_template(TemplateKind::ImagePrompt)
        );
    }

    #[test]
    fn an_untouched_earlier_default_moves_on_to_the_format_aware_one() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let app = start(&db);
        let kinds = [TemplateKind::Script, TemplateKind::ImagePrompt];
        for kind in kinds {
            db.add_template_version(&TemplateVersion::first(
                app.profile().id,
                retired_defaults(kind).remove(0),
                SystemTime::now(),
            ))
            .unwrap();
        }
        for kind in kinds {
            let retired = retired_defaults(kind).remove(0);
            let versions = app.template_versions(kind).unwrap();
            assert_eq!(versions.len(), 2);
            assert_eq!(versions[0].number, 2);
            assert_eq!(versions[0].body, default_template(kind));
            assert_eq!(versions[1].body, retired, "the old version stays readable");
            assert_eq!(app.template_versions(kind).unwrap().len(), 2, "once");
        }
    }

    #[test]
    fn each_part_still_at_an_earlier_default_moves_on_by_itself() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let app = start(&db);
        let kind = TemplateKind::Script;
        let retired = retired_defaults(kind).remove(0);
        let edited = TemplateBody::new(kind, "My own rules.", retired.prompt()).unwrap();
        db.add_template_version(&TemplateVersion::first(
            app.profile().id,
            edited,
            SystemTime::now(),
        ))
        .unwrap();
        let current = app.current_template(kind).unwrap();
        assert_eq!(current.number, 2);
        assert_eq!(current.body.instructions(), "My own rules.");
        assert_eq!(current.body.prompt(), default_template(kind).prompt());
    }

    #[test]
    fn an_earlier_default_put_back_by_hand_stays() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let app = start(&db);
        let kind = TemplateKind::Script;
        let retired = retired_defaults(kind).remove(0);
        db.add_template_version(&TemplateVersion::first(
            app.profile().id,
            retired.clone(),
            SystemTime::now(),
        ))
        .unwrap();
        assert_eq!(app.current_template(kind).unwrap().number, 2);
        let restored = app
            .save_template(kind, retired.instructions(), retired.prompt())
            .unwrap();
        assert_eq!(restored.number, 3);
        assert_eq!(app.current_template(kind).unwrap().id, restored.id);
    }

    #[test]
    fn an_edited_template_is_left_as_the_user_wrote_it() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let app = start(&db);
        let edited = app
            .save_template(
                TemplateKind::Script,
                "Rules.",
                "A 9-minute script about {{niche}}.",
            )
            .unwrap();
        assert_eq!(
            app.current_template(TemplateKind::Script).unwrap().id,
            edited.id
        );
    }

    #[test]
    fn the_defaults_take_their_length_and_frame_from_the_format() {
        let script = default_template(TemplateKind::Script);
        assert!(script.prompt().contains("{{target_length}}"));
        assert!(!script.prompt().contains("minutes spoken"));
        let scenes = default_template(TemplateKind::ImagePrompt);
        assert!(scenes.instructions().contains("{{frame_shape}}"));
        assert!(!scenes.instructions().contains("16:9"));
    }

    #[test]
    fn saving_adds_a_version_and_old_ones_stay_readable() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let app = start(&db);
        let first = app.current_template(TemplateKind::Script).unwrap();

        let second = app
            .save_template(TemplateKind::Script, "Short rules.", "About {{niche}}.")
            .unwrap();
        assert_eq!(second.number, 2);
        assert_eq!(
            app.current_template(TemplateKind::Script).unwrap().id,
            second.id
        );

        let numbers: Vec<_> = app
            .template_versions(TemplateKind::Script)
            .unwrap()
            .iter()
            .map(|version| version.number)
            .collect();
        assert_eq!(numbers, [2, 1], "newest first");
        assert_eq!(app.template_version(first.id).unwrap(), first);
    }

    #[test]
    fn saving_unchanged_text_adds_no_version() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let app = start(&db);
        let current = app.current_template(TemplateKind::Script).unwrap();
        let saved = app
            .save_template(
                TemplateKind::Script,
                &format!(" {}", current.body.instructions()),
                current.body.prompt(),
            )
            .unwrap();
        assert_eq!(saved, current);
        assert_eq!(
            app.template_versions(TemplateKind::Script).unwrap().len(),
            1
        );
    }

    #[test]
    fn invalid_text_is_rejected_field_by_field() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let app = start(&db);
        let error = app
            .save_template(TemplateKind::Script, "Use {{voice}}.", " ")
            .unwrap_err();
        assert_eq!(
            error.field_errors(),
            [
                TemplateFieldError {
                    field: TemplateField::Instructions,
                    problem: TemplateProblem::UnknownVariable,
                },
                TemplateFieldError {
                    field: TemplateField::Prompt,
                    problem: TemplateProblem::Required,
                },
            ]
        );
        assert_eq!(
            error.message(),
            Text::TemplateProblem(TemplateProblem::UnknownVariable)
        );
        assert_eq!(
            app.template_versions(TemplateKind::Script).unwrap().len(),
            1
        );
    }

    #[test]
    fn other_profiles_versions_are_not_found() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let app = start(&db);
        let stranger = bardo_domain::UserProfile::new(bardo_domain::UiLanguage::EnUs);
        bardo_domain::ProfileRepository::save(&*db, &stranger).unwrap();
        let foreign = TemplateVersion::first(
            stranger.id,
            default_template(TemplateKind::Script),
            SystemTime::now(),
        );
        db.add_template_version(&foreign).unwrap();
        assert!(matches!(
            app.template_version(foreign.id),
            Err(TemplateError::NotFound)
        ));
    }
}
