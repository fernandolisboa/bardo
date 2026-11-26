//! Publishing an ad (docs/spec/marketing-videos.md, "Publishing an ad";
//! ADR-0045): its commercial disclosure by default, the check of its post
//! texts, and what the user acknowledges before an upload or an export of
//! it starts.
//!
//! Bardo sets defaults and reminds; the user decides each disclosure.
//! Projects that are not ads have none of this, and their reviews and
//! exports are as before.

use bardo_domain::{
    AdReviewItem, CheckedScript, ClaimDrift, ClaimsCheck, CommercialDisclosure, DisclosureStep,
    Job, JobId, JobKind, JobState, Network, Provider, RepositoryError, SceneKind, ScriptText,
    VideoProject, VideoProjectId, ad_synthetic_default, disclosure_step,
};

use crate::ad_briefs::ScriptBrief;
use crate::claims_check::{CheckBrief, CheckPayload, UnbackedStatement, millis, posts_of};
use crate::costs::{BudgetConsent, SpendEstimate};
use crate::jobs::Work;
use crate::{Bardo, ExportError, KeyState, Text};

/// The check of an ad's post texts, as the Publishing stage shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostsCheckView {
    /// The texts or the brief changed since: check again.
    pub outdated: bool,
    /// What the texts state that no claim backs.
    pub unbacked: Vec<UnbackedStatement>,
    pub checked_at: std::time::SystemTime,
}

/// An ad project's publishing: what its reviews start with and ask for.
#[derive(Debug, Clone, PartialEq)]
pub struct AdPublishing {
    /// The commercial disclosure its posts start with, from whose the
    /// product is.
    pub disclosure: CommercialDisclosure,
    /// What the user acknowledges before an upload or an export starts.
    pub items: Vec<AdReviewItem>,
    /// The check of the post texts; `None` until they are checked.
    pub posts: Option<PostsCheckView>,
    /// Whether there are post texts to check.
    pub has_posts: bool,
    /// What checking the post texts would cost.
    pub check_estimate: Option<SpendEstimate>,
    /// A check of the post texts asked for on its own is at work.
    pub checking: bool,
    /// The last check of the post texts asked for on its own stopped
    /// (failed or cancelled) and none was saved after it.
    pub check_stopped: bool,
}

impl AdPublishing {
    /// The keys of every item: what acknowledging them all names.
    pub fn keys(&self) -> Vec<String> {
        self.items.iter().map(AdReviewItem::key).collect()
    }

    /// What reviews compare: a changed default or item needs a new review.
    pub(crate) fn stamp(&self) -> String {
        format!(
            "ad:{}:{}",
            self.disclosure.code(),
            self.keys().join("\u{1f}")
        )
    }
}

/// Whether the saved check `check` read `text` as the brief stands now.
fn current(check: &ClaimsCheck, text: &ScriptText, brief: &ScriptBrief, notes: &str) -> bool {
    !check.is_outdated(text, &brief.brief, notes)
}

/// The items `check` of `of` adds: what it found unbacked and the
/// sensitive claims it found used, or that it is not up to date.
fn check_items(
    check: Option<&ClaimsCheck>,
    of: CheckedScript,
    text: &ScriptText,
    brief: &ScriptBrief,
    notes: &str,
    items: &mut Vec<AdReviewItem>,
) {
    let Some(check) = check.filter(|check| current(check, text, brief, notes)) else {
        items.push(AdReviewItem::NotChecked(of));
        return;
    };
    for statement in check.unbacked() {
        items.push(AdReviewItem::Unbacked {
            of,
            says: statement.says.clone(),
        });
    }
    for id in check.claims_used() {
        if let Some(copy) = brief.brief.claims().iter().find(|copy| copy.claim == id)
            && copy.sensitive_now(&brief.sheet)
        {
            items.push(AdReviewItem::SensitiveClaim(copy.text.clone()));
        }
    }
}

impl Bardo {
    /// Whether the cut of an ad shows a *product in use* scene drawn in a
    /// photographic style, or in one Bardo cannot tell: its posts start
    /// with the synthetic-content disclosure on.
    pub(crate) fn ad_needs_synthetic(
        &self,
        project: &VideoProject,
    ) -> Result<bool, RepositoryError> {
        if !project.is_ad() {
            return Ok(false);
        }
        let in_use = self
            .scene_plans
            .scene_plan(project.id)?
            .is_some_and(|plan| {
                plan.scenes()
                    .iter()
                    .any(|scene| scene.kind() == SceneKind::ProductInUse)
            });
        Ok(ad_synthetic_default(in_use, self.video_style(project)?))
    }

    /// The publishing of an ad project; `None` for any other project, and
    /// for an ad whose brief or product is gone.
    pub(crate) fn ad_publishing(
        &self,
        project: &VideoProject,
    ) -> Result<Option<AdPublishing>, ExportError> {
        let Some(brief) = self.script_brief(project)? else {
            return Ok(None);
        };
        let reads = CheckBrief::of(&brief);
        let mut items = Vec::new();
        if let Some(script) = self.scripts.script(project.id)? {
            let check = self.ads.claims_check(project.id, CheckedScript::Current)?;
            check_items(
                check.as_ref().filter(|check| check.reads(script.text())),
                CheckedScript::Current,
                script.text(),
                &brief,
                &reads.notes,
                &mut items,
            );
        }
        let posts_text = posts_of(&self.exports.video_metadata(project.id)?);
        let posts_check = self.ads.claims_check(project.id, CheckedScript::Posts)?;
        let mut posts = None;
        if let Some(text) = &posts_text {
            let check = posts_check.as_ref().filter(|check| check.reads(text));
            check_items(
                check,
                CheckedScript::Posts,
                text,
                &brief,
                &reads.notes,
                &mut items,
            );
            posts = posts_check.as_ref().map(|check| PostsCheckView {
                outdated: !check.reads(text) || !current(check, text, &brief, &reads.notes),
                unbacked: check
                    .unbacked()
                    .map(|statement| UnbackedStatement {
                        sentence: check.sentence_of(statement).map(str::to_owned),
                        says: statement.says.clone(),
                    })
                    .collect(),
                checked_at: check.checked_at,
            });
        }
        for copy in brief.brief.claims() {
            if copy.drift(&brief.sheet) == Some(ClaimDrift::Unconfirmed) {
                items.push(AdReviewItem::Unconfirmed(copy.text.clone()));
            }
        }
        if let Some(category) = brief.sheet.text().restricted {
            items.push(AdReviewItem::RestrictedCategory(category));
        }
        if brief.is_concept() {
            items.push(AdReviewItem::Concept);
        }
        // Each item once: a claim both texts use is acknowledged once.
        let mut unique: Vec<AdReviewItem> = Vec::new();
        for item in items {
            if !unique.iter().any(|kept| kept.key() == item.key()) {
                unique.push(item);
            }
        }
        // The last check asked for on its own, and whether it stopped with
        // no check saved after it (by itself or by a later metadata job).
        let id = project.id.to_string();
        let jobs = self.jobs();
        let last = jobs
            .iter()
            .rposition(|job| Bardo::checks_posts_of(job, &id));
        let last_check = last.map(|at| jobs[at].clone());
        let saved_by = posts_check
            .as_ref()
            .and_then(|check| check.job)
            .and_then(|saver| jobs.iter().position(|job| job.id() == saver));
        let check_stopped = last.is_some_and(|at| {
            let job = &jobs[at];
            matches!(job.state(), JobState::Failed | JobState::Cancelled)
                && !self.at_work(job)
                && saved_by.is_none_or(|saved| saved < at)
        });
        let check_estimate = match &posts_text {
            Some(text) => Some(self.estimate(&[reads.call(text, CheckedScript::Posts)])?),
            None => None,
        };
        Ok(Some(AdPublishing {
            disclosure: CommercialDisclosure::default_for(brief.product.brief.ownership()),
            items: unique,
            posts,
            has_posts: posts_text.is_some(),
            check_estimate,
            checking: last_check.as_ref().is_some_and(|job| self.at_work(job)),
            check_stopped,
        }))
    }

    /// Starts a job in which Claude checks the post texts of an ad
    /// against the claims of its Brief stage. The new check replaces the
    /// texts' earlier one. Past Claude's budget it needs `consent`.
    pub fn check_post_claims(
        &self,
        project: VideoProjectId,
        consent: BudgetConsent,
    ) -> Result<JobId, ExportError> {
        let project = self
            .themes
            .project(project)?
            .filter(|project| project.owner == self.profile.id)
            .ok_or(ExportError::ProjectNotFound)?;
        let Some(brief) = self.script_brief(&project)? else {
            return Err(ExportError::NotAnAd);
        };
        let text =
            posts_of(&self.exports.video_metadata(project.id)?).ok_or(ExportError::NoMetadata)?;
        if self.work_busy(&Work::Metadata(project.id.to_string())) {
            return Err(ExportError::Busy);
        }
        if self.provider_key(Provider::Claude).state == KeyState::NotSet {
            return Err(ExportError::MissingKey(Provider::Claude));
        }
        let check = CheckBrief::of(&brief);
        if let Err(estimate) = self.check_video_budget(
            project.id,
            &[check.call(&text, CheckedScript::Posts)],
            consent,
        )? {
            return Err(ExportError::OverBudget(estimate));
        }
        let payload =
            CheckPayload::posts(project.id, &text, millis(brief.brief.changed_at()), check);
        let job = Job::new(
            self.profile.id,
            JobKind::ClaimsCheck,
            serde_json::to_string(&payload).expect("a claims check payload serializes"),
        );
        Ok(self.jobs.enqueue(job)?)
    }

    /// One acknowledgement item as the review lists it.
    pub fn ad_item_text(&self, item: &AdReviewItem) -> String {
        match item {
            AdReviewItem::SensitiveClaim(text) => {
                self.text_with(Text::AdItemSensitive, &[("claim", text)])
            }
            AdReviewItem::Unbacked { of, says } => self.text_with(
                match of {
                    CheckedScript::Posts => Text::AdItemUnbackedPosts,
                    CheckedScript::Current | CheckedScript::Pending => Text::AdItemUnbackedScript,
                },
                &[("says", says)],
            ),
            AdReviewItem::NotChecked(of) => self
                .text(match of {
                    CheckedScript::Posts => Text::AdItemPostsNotChecked,
                    CheckedScript::Current | CheckedScript::Pending => Text::AdItemScriptNotChecked,
                })
                .into_owned(),
            AdReviewItem::Unconfirmed(text) => {
                self.text_with(Text::AdItemUnconfirmed, &[("claim", text)])
            }
            AdReviewItem::RestrictedCategory(category) => self.text_with(
                Text::AdItemCategory,
                &[(
                    "category",
                    &self.text(Text::RestrictedCategoryName(*category)),
                )],
            ),
            AdReviewItem::Concept => self.text(Text::AdItemConcept).into_owned(),
        }
    }

    /// What disclosing `disclosure` on `network` asks of the user, as one
    /// sentence; `None` when nothing does.
    pub fn commercial_step_text(
        &self,
        network: Network,
        disclosure: CommercialDisclosure,
    ) -> Option<String> {
        let paid = disclosure.is_paid();
        let text = match disclosure_step(network, disclosure) {
            DisclosureStep::Nothing => return None,
            DisclosureStep::Sent => Text::CommercialSent(network),
            DisclosureStep::InApp if paid => Text::CommercialTikTokBranded,
            DisclosureStep::InApp => Text::CommercialTikTokYourBrand,
        };
        Some(self.text(text).into_owned())
    }

    /// The reminder line of an export's metadata file: the disclosure, and
    /// what to set by hand on `network` when posting it, since an export
    /// is posted by hand and carries no label.
    pub fn commercial_reminder(
        &self,
        network: Network,
        disclosure: CommercialDisclosure,
    ) -> String {
        let step = match (disclosure_step(network, disclosure), disclosure.is_paid()) {
            (DisclosureStep::Sent, _) => Text::CommercialReminderLabel,
            (DisclosureStep::InApp, true) => Text::CommercialReminderTikTokBranded,
            (DisclosureStep::InApp, false) => Text::CommercialReminderTikTokYourBrand,
            (DisclosureStep::Nothing, true) => Text::CommercialReminderNoLabel,
            (DisclosureStep::Nothing, false) => Text::CommercialReminderNothing,
        };
        let network = self.text(Text::NetworkName(network));
        format!(
            "{} {}",
            self.text_with(
                Text::CommercialReminder,
                &[("disclosure", &self.text(Text::CommercialName(disclosure)))],
            ),
            self.text_with(step, &[("network", &network)])
        )
    }

    /// The YouTube Studio page of an uploaded video, where a paid
    /// partnership's *Includes paid promotion* shows.
    pub fn studio_link(video_id: &str) -> String {
        format!("https://studio.youtube.com/video/{video_id}/edit")
    }
}

#[cfg(test)]
mod tests {
    use bardo_domain::{
        AdReviewItem, CheckedScript, CommercialDisclosure, METADATA_FILE, Network,
        NetworkAccountDraft, ProductOwnership, StyleChoice, TemplateKind, VideoMetadataDraft,
        VisualStyle,
    };

    use std::sync::Arc;
    use std::time::{Duration, SystemTime};

    use bardo_domain::{
        ConnectedIdentity, ConnectionSecrets, ConnectionStatus, NetworkAccountRepository,
        NetworkConnection, NetworkConnectionRepository, SecretText, TokenGrant, TokenSet,
    };
    use bardo_storage::MemoryExportFiles;

    use super::*;
    use crate::ad_angles::testing::{TYPESAFE_KEY, draft};
    use crate::ad_scenes::ad_tests::{A_SCREEN_AND_A_PHOTO, answer, narrated_ad, plan_with};
    use crate::claims_check::POSTS_CHECK_INSTRUCTIONS;
    use crate::export::tests::add_account;
    use crate::render::tests::render_all;
    use crate::scenes::tests::{Harness, done};
    use crate::{AdUploadChoices, UploadReviewError};

    /// Bardo with the keys an ad needs, its exports written to `files`.
    fn start(h: &Harness, files: &Arc<MemoryExportFiles>) -> Bardo {
        let mut app = h.start_with_exports(Arc::clone(files) as _);
        app.save_provider_key(Provider::TypeSafe, TYPESAFE_KEY)
            .unwrap();
        app
    }

    /// Connects the project's account on `network`.
    fn connect(h: &Harness, app: &Bardo, project: &VideoProject, network: Network) {
        let account = NetworkAccountRepository::list(&*h.db, project.channel)
            .unwrap()
            .into_iter()
            .find(|account| account.network == network)
            .unwrap();
        let now = SystemTime::now();
        let tokens = TokenSet::granted(
            &TokenGrant {
                access_token: SecretText::new("ya29.upload-access"),
                refresh_token: Some(SecretText::new("1//refresh")),
                expires_in: Duration::from_secs(3600),
                scopes: Vec::new(),
            },
            now,
        );
        h.connection_secrets
            .set_tokens(app.profile().id, account.id, &tokens)
            .unwrap();
        NetworkConnectionRepository::save(
            &*h.db,
            &NetworkConnection {
                account: account.id,
                owner: app.profile().id,
                status: ConnectionStatus::Connected,
                identity: ConnectedIdentity {
                    id: "UCdesk".into(),
                    name: "Study Desk".into(),
                },
                scopes: Vec::new(),
                expires_at: tokens.expires_at(),
                connected_at: now,
                refreshed_at: None,
            },
        )
        .unwrap();
    }

    /// Each network's post of the ad: the hook, the claims and the call
    /// to action, with the link where the network shows links.
    fn posts() -> String {
        serde_json::json!({ "posts": [
            {
                "network": "youtube",
                "title": "The notebook that refills",
                "description": "Tired of buying notebooks? This one refills. Get yours at example.com/notebook.",
                "tags": ["notebook", "study"]
            },
            {
                "network": "tiktok",
                "title": "",
                "description": "Tired of buying notebooks? It lasts 3 school years. Link in bio.",
                "tags": ["notebook"]
            },
            {
                "network": "instagram_reels",
                "title": "",
                "description": "This one refills. Get yours: link in bio.",
                "tags": ["notebook"]
            }
        ]})
        .to_string()
    }

    /// An ad with its script and narration, planned without a *product in
    /// use* scene, and YouTube, TikTok and Instagram accounts.
    fn ad(h: &Harness, app: &Bardo) -> VideoProject {
        let (_, project, _) = narrated_ad(h, app, &A_SCREEN_AND_A_PHOTO);
        for network in [Network::YouTube, Network::TikTok, Network::InstagramReels] {
            add_account(app, &project, network, NetworkAccountDraft::default());
        }
        project
    }

    /// `ad`'s metadata written, its post texts checked with `statements`.
    fn written(h: &Harness, app: &Bardo, project: &VideoProject, statements: &[(i64, &str, i64)]) {
        h.answer(posts());
        h.text.check_with(statements);
        done(
            app,
            app.generate_metadata(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
    }

    fn publishing(app: &Bardo, project: &VideoProject) -> AdPublishing {
        app.export_view(project.id).unwrap().ad.unwrap()
    }

    #[test]
    fn an_ads_metadata_comes_from_its_template_with_the_rules_and_checks_its_posts() {
        let h = Harness::new();
        let files: Arc<MemoryExportFiles> = Arc::default();
        let app = start(&h, &files);
        let project = ad(&h, &app);
        let view = app.export_view(project.id).unwrap();
        assert_eq!(view.template.body.kind(), TemplateKind::AdMetadata);
        written(
            &h,
            &app,
            &project,
            &[
                (2, "This one refills", 1),
                (5, "It lasts 3 school years", 3),
            ],
        );

        let request = h
            .text
            .requests()
            .into_iter()
            .rev()
            .find(|request| request.prompt.contains("The networks, one per line"))
            .unwrap();
        assert!(request.instructions.ends_with(crate::ads::AD_RULES));
        assert!(request.instructions.contains("Open with the hook"));
        assert!(
            request
                .prompt
                .contains("1. Refillable pages\n2. A5 size\n3. Lasts 3 school years"),
            "{}",
            request.prompt
        );
        assert!(
            request
                .prompt
                .contains("Link: https://example.com/notebook"),
            "{}",
            request.prompt
        );
        assert!(request.prompt.contains("<never_say>\ncheap\n</never_say>"));

        let check = h.text.check_requests().pop().unwrap();
        assert_eq!(check.instructions, POSTS_CHECK_INSTRUCTIONS);
        assert!(
            check
                .prompt
                .contains("<posts>\n1. YouTube: The notebook that refills.")
        );
        assert!(
            check
                .prompt
                .contains("4. Get yours at example.com/notebook.")
        );
        assert!(
            !check.prompt.contains("study"),
            "tags state nothing and are not checked: {}",
            check.prompt
        );

        let ad = publishing(&app, &project);
        let posts = ad.posts.clone().unwrap();
        assert!(!posts.outdated);
        assert!(posts.unbacked.is_empty());
        assert!(
            ad.items
                .contains(&AdReviewItem::SensitiveClaim("Lasts 3 school years".into())),
            "{:?}",
            ad.items
        );
        assert!(
            !ad.items
                .contains(&AdReviewItem::NotChecked(CheckedScript::Posts))
        );
    }

    #[test]
    fn an_edit_to_the_post_texts_puts_their_check_out_of_date_until_checked_again() {
        let h = Harness::new();
        let files: Arc<MemoryExportFiles> = Arc::default();
        let app = start(&h, &files);
        let project = ad(&h, &app);
        written(&h, &app, &project, &[(2, "This one refills", 1)]);
        app.edit_metadata(
            project.id,
            Network::InstagramReels,
            VideoMetadataDraft {
                title: String::new(),
                description: "The best notebook ever made. Link in bio.".into(),
                tags: vec!["notebook".into()],
            },
        )
        .unwrap();
        let ad = publishing(&app, &project);
        assert!(ad.posts.as_ref().unwrap().outdated);
        assert!(
            ad.items
                .contains(&AdReviewItem::NotChecked(CheckedScript::Posts))
        );

        h.text.check_with(&[(5, "The best notebook ever made", 0)]);
        done(
            &app,
            app.check_post_claims(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        let ad = publishing(&app, &project);
        assert!(!ad.posts.as_ref().unwrap().outdated);
        assert!(ad.items.contains(&AdReviewItem::Unbacked {
            of: CheckedScript::Posts,
            says: "The best notebook ever made".into(),
        }));
        // Writing the metadata again leaves the new texts unchecked until
        // its own check, which the job makes.
        written(&h, &app, &project, &[]);
        let ad = publishing(&app, &project);
        assert!(ad.posts.as_ref().unwrap().unbacked.is_empty());
    }

    #[test]
    fn the_disclosure_starts_from_whose_the_product_is() {
        let h = Harness::new();
        let files: Arc<MemoryExportFiles> = Arc::default();
        let app = start(&h, &files);
        let project = ad(&h, &app);
        assert_eq!(
            publishing(&app, &project).disclosure,
            CommercialDisclosure::YourBrand
        );
        let product = project.product.unwrap();
        app.update_product(
            product,
            bardo_domain::ProductDraft {
                ownership: ProductOwnership::Client,
                ..draft()
            },
        )
        .unwrap();
        assert_eq!(
            publishing(&app, &project).disclosure,
            CommercialDisclosure::PaidPartnership
        );
    }

    #[test]
    fn a_product_in_use_drawn_like_a_photo_turns_the_synthetic_default_on() {
        let h = Harness::new();
        let files: Arc<MemoryExportFiles> = Arc::default();
        let app = start(&h, &files);
        let project = ad(&h, &app);
        app.set_project_style(
            project.id,
            Some(StyleChoice::Catalog(VisualStyle::Watercolor)),
        )
        .unwrap();
        plan_with(
            &h,
            &app,
            &project,
            answer([
                ("drawn", -1),
                ("product_shot", 1),
                ("product_in_use", -1),
                ("end_card", 0),
            ]),
        );
        let synthetic = |app: &Bardo| app.export_view(project.id).unwrap().disclosure;
        assert!(!synthetic(&app), "a watercolor is not a photograph");
        app.set_project_style(
            project.id,
            Some(StyleChoice::Catalog(VisualStyle::Photorealistic)),
        )
        .unwrap();
        assert!(synthetic(&app));
        app.set_project_style(project.id, Some(StyleChoice::FromNotes))
            .unwrap();
        assert!(synthetic(&app), "a style Bardo cannot tell");
    }

    #[test]
    fn an_ads_export_waits_for_every_acknowledgement_and_writes_the_reminders() {
        let h = Harness::new();
        let files: Arc<MemoryExportFiles> = Arc::default();
        let app = start(&h, &files);
        let project = ad(&h, &app);
        plan_with(
            &h,
            &app,
            &project,
            answer([
                ("drawn", -1),
                ("product_shot", 1),
                ("drawn", -1),
                ("end_card", 0),
            ]),
        );
        done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        app.open_editor(project.id).unwrap();
        render_all(&app, project.id);
        written(&h, &app, &project, &[(5, "It lasts 3 school years", 3)]);
        let view = app.export_view(project.id).unwrap();
        let ad = view.ad.clone().unwrap();
        assert!(!ad.items.is_empty());
        assert!(matches!(
            app.start_export(&view, &[Network::TikTok]),
            Err(ExportError::NotAcknowledged)
        ));
        let mut some = ad.keys();
        some.pop();
        assert!(matches!(
            app.start_ad_export(&view, &[Network::TikTok], &some),
            Err(ExportError::NotAcknowledged)
        ));
        done(
            &app,
            app.start_ad_export(&view, &[Network::TikTok], &ad.keys())
                .unwrap(),
        );
        let saved = app
            .exports
            .export_ad_review(project.id, Network::TikTok)
            .unwrap()
            .unwrap();
        assert_eq!(saved.disclosure, CommercialDisclosure::YourBrand);
        assert_eq!(saved.acknowledged, ad.keys());
        let text = String::from_utf8(
            files
                .read(&view.package, Network::TikTok, METADATA_FILE)
                .unwrap(),
        )
        .unwrap();
        assert!(
            text.contains(
                "Commercial disclosure: Your brand. Before posting, turn on \"Disclose post content\" in TikTok and pick \"Your brand\"."
            ),
            "{text}"
        );
        assert!(text.contains("Before posting this ad, keep in mind:"));
        assert!(text.contains("Sensitive claim used: \"Lasts 3 school years\""));
        let view = app.export_view(project.id).unwrap();
        assert!(
            view.targets
                .iter()
                .find(|target| target.network == Network::TikTok)
                .unwrap()
                .last_current
        );
    }

    #[test]
    fn an_ads_upload_waits_for_its_acknowledgements_and_keeps_its_review() {
        let h = Harness::new();
        let files: Arc<MemoryExportFiles> = Arc::default();
        let app = start(&h, &files);
        let project = ad(&h, &app);
        plan_with(
            &h,
            &app,
            &project,
            answer([
                ("drawn", -1),
                ("product_shot", 1),
                ("drawn", -1),
                ("end_card", 0),
            ]),
        );
        done(
            &app,
            app.generate_scene_images(project.id, BudgetConsent::Ask)
                .unwrap(),
        );
        app.open_editor(project.id).unwrap();
        render_all(&app, project.id);
        written(&h, &app, &project, &[(5, "It lasts 3 school years", 3)]);
        connect(&h, &app, &project, Network::YouTube);
        let review = app.upload_review(project.id, Network::YouTube).unwrap();
        let ad = review.ad.clone().unwrap();
        assert!(!ad.items.is_empty());
        let refused = app.start_upload(&review, review.choices());
        assert!(
            matches!(refused, Err(UploadReviewError::NotAcknowledged)),
            "{refused:?} {:?}",
            review.block()
        );
        let choices = AdUploadChoices {
            disclosure: Some(CommercialDisclosure::PaidPartnership),
            sponsors: "@not-an-id".into(),
            acknowledged: ad.keys(),
        };
        // YouTube takes the paid promotion with the upload; the brands
        // typed are Instagram's and are not read.
        let job = app
            .start_ad_upload(&review, review.choices(), &choices)
            .unwrap();
        let payload = app
            .jobs()
            .into_iter()
            .find(|queued| queued.id() == job)
            .unwrap()
            .payload()
            .to_owned();
        assert!(payload.contains("\"paid_partnership\":true"), "{payload}");
        assert!(!payload.contains("sponsors"), "{payload}");
        let publication = app
            .publications
            .publications(project.id)
            .unwrap()
            .into_iter()
            .find(|publication| publication.network() == Network::YouTube)
            .unwrap();
        let kept = app.publications.ad_review(publication.id).unwrap().unwrap();
        assert_eq!(kept.disclosure, CommercialDisclosure::PaidPartnership);
        assert!(kept.sponsors.is_empty());
        assert_eq!(kept.acknowledged, ad.keys());
        assert!(
            app.commercial_step_text(Network::YouTube, kept.disclosure)
                .unwrap()
                .contains("Includes paid promotion")
        );

        // Once YouTube made the video, the review links its Studio page.
        let review = app.upload_review(project.id, Network::YouTube).unwrap();
        let uploaded = review.replaces.as_ref().unwrap();
        assert_eq!(uploaded.id, publication.id);
        let video = uploaded.post_id().expect("the fake upload made the video");
        assert_eq!(
            review.studio_link,
            Some(format!("https://studio.youtube.com/video/{video}/edit"))
        );
    }

    #[test]
    fn a_project_that_is_no_ad_has_no_ad_publishing() {
        let s = crate::export::tests::rendered();
        let view = s.app.export_view(s.project.id).unwrap();
        assert_eq!(view.ad, None);
        assert_eq!(view.template.body.kind(), TemplateKind::Metadata);
    }

    #[test]
    fn an_exports_reminder_says_what_to_turn_on_by_hand() {
        let h = Harness::new();
        let app = h.start();
        let paid = CommercialDisclosure::PaidPartnership;
        let brand = CommercialDisclosure::YourBrand;
        assert_eq!(
            app.commercial_reminder(Network::YouTube, paid),
            "Commercial disclosure: Paid partnership. Turn on YouTube's paid partnership or paid promotion label when you post."
        );
        assert_eq!(
            app.commercial_reminder(Network::InstagramReels, brand),
            "Commercial disclosure: Your brand. Instagram Reels asks nothing more for it."
        );
        assert!(
            app.commercial_reminder(Network::TikTok, paid)
                .contains("pick \"Branded content\"")
        );
        assert_eq!(
            app.commercial_reminder(Network::Kick, paid),
            "Commercial disclosure: Paid partnership. Kick has no label for it: say so in the post."
        );
    }

    #[test]
    fn each_network_asks_for_what_it_documents() {
        let h = Harness::new();
        let app = h.start();
        let paid = CommercialDisclosure::PaidPartnership;
        let brand = CommercialDisclosure::YourBrand;
        assert_eq!(app.commercial_step_text(Network::YouTube, brand), None);
        assert_eq!(app.commercial_step_text(Network::X, brand), None);
        assert!(
            app.commercial_step_text(Network::TikTok, brand)
                .unwrap()
                .contains("\"Your brand\"")
        );
        let branded = app.commercial_step_text(Network::TikTok, paid).unwrap();
        assert!(branded.contains("\"Branded content\""));
        assert!(branded.contains("private post"));
        assert!(
            app.commercial_step_text(Network::InstagramReels, paid)
                .unwrap()
                .contains("never posts without the label")
        );
        assert_eq!(
            app.commercial_step_text(Network::Kick, paid),
            None,
            "Kick's export carries the reminder"
        );
        assert_eq!(
            Bardo::studio_link("dQw4w9WgXcQ"),
            "https://studio.youtube.com/video/dQw4w9WgXcQ/edit"
        );
    }
}
