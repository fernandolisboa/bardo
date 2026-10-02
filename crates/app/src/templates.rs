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

Write the narration script for this video, about 1,200 to 1,500 words (8 to 10 minutes spoken). \
Hook the viewer in the first two sentences, keep the tension through the middle, and end with a \
payoff that delivers on the title's promise.";

const IMAGE_PROMPT_INSTRUCTIONS: &str = "\
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

/// Bardo's own text for `kind`: every profile's version 1.
pub fn default_template(kind: TemplateKind) -> TemplateBody {
    let (instructions, prompt) = match kind {
        TemplateKind::Script => (SCRIPT_INSTRUCTIONS, SCRIPT_PROMPT),
        TemplateKind::ImagePrompt => (IMAGE_PROMPT_INSTRUCTIONS, IMAGE_PROMPT_PROMPT),
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
