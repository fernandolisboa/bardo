//! The Brief stage of an ad project: what this one video says about its
//! product. The brief starts, when an angle is approved, with a copy of
//! every claim the product's sheet confirms and the product's own offer,
//! call to action and link. The user switches claims off for this video,
//! edits the words and confirms it; the Script stage opens then.
//!
//! The copies are the video's own. Editing or rejecting a claim in the
//! product never changes a video: the stage shows Attention instead, and
//! the user keeps or drops each copy. A change to the brief after a
//! script was written marks the script until the user says it still
//! holds or writes it again.

use std::time::SystemTime;

use bardo_domain::{
    AdAngle, AdBrief, BriefClaim, BriefClaimError, BriefFieldError, BriefNotReady, BriefText,
    Claim, ClaimDrift, ClaimId, Product, ProductId, ProductSheet, RepositoryError, Script,
    VideoProject, VideoProjectId,
};

use crate::{Bardo, Text};

#[derive(Debug, thiserror::Error)]
pub enum BriefError {
    #[error("video project not found")]
    ProjectNotFound,
    /// The project advertises no product: it has no Brief stage.
    #[error("the project is not an ad")]
    NotAnAd,
    /// The typed text breaks one or more rules; the form shows each one.
    #[error("invalid brief: {0:?}")]
    Invalid(Vec<BriefFieldError>),
    /// The brief cannot be confirmed yet.
    #[error("the brief is not ready: {0:?}")]
    NotReady(BriefNotReady),
    #[error(transparent)]
    Claim(#[from] BriefClaimError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

impl BriefError {
    /// What the Brief stage says. Field errors are shown next to their
    /// fields.
    pub fn message(&self) -> Option<Text> {
        match self {
            BriefError::ProjectNotFound => Some(Text::ProjectNotFound),
            BriefError::NotAnAd => Some(Text::BriefNotAnAd),
            BriefError::Invalid(_) => None,
            BriefError::NotReady(reason) => Some(Text::BriefNotReady(*reason)),
            BriefError::Claim(BriefClaimError::NotConfirmed) => Some(Text::BriefClaimNotConfirmed),
            BriefError::Claim(BriefClaimError::NotUsed) => Some(Text::BriefClaimNotUsed),
            BriefError::Repository(_) => Some(Text::BriefNotSaved),
        }
    }

    pub fn field_errors(&self) -> &[BriefFieldError] {
        match self {
            BriefError::Invalid(errors) => errors,
            _ => &[],
        }
    }
}

/// Where an ad project's brief stands, as the project's stages read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BriefStanding {
    /// The user confirmed it: the Script stage is open.
    pub confirmed: bool,
    /// Nothing stands in the way of confirming it.
    pub ready: bool,
    /// Copies whose claim changed in the product, waiting to be kept or
    /// dropped.
    pub to_decide: usize,
    /// The claims the video uses.
    pub claims: usize,
    /// The script was written from an earlier brief, and the user has not
    /// said that it still holds.
    pub script_outdated: bool,
}

/// One claim the video uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BriefClaimRow {
    /// The video's copy.
    pub copy: BriefClaim,
    /// How the product's claim differs from the copy now, when the user
    /// has yet to keep or drop it.
    pub drift: Option<ClaimDrift>,
    /// Whether the project's angle leans on it.
    pub in_angle: bool,
}

/// An ad project's Brief stage.
#[derive(Debug, Clone, PartialEq)]
pub struct BriefView {
    pub project: VideoProject,
    pub product: Product,
    /// The angle the project was approved from.
    pub angle: Option<AdAngle>,
    pub brief: AdBrief,
    /// The claims the video uses, in the brief's order.
    pub claims: Vec<BriefClaimRow>,
    /// The product's confirmed claims the video does not use: switched
    /// off, or confirmed after the project started.
    pub available: Vec<Claim>,
    pub standing: BriefStanding,
    /// What stands in the way of confirming, the first of them.
    pub not_ready: Option<BriefNotReady>,
}

/// Where `brief` stands against what the product says now and the script
/// written so far.
fn standing(brief: &AdBrief, sheet: &ProductSheet, script: Option<&Script>) -> BriefStanding {
    BriefStanding {
        confirmed: brief.is_confirmed(),
        ready: brief.readiness(sheet).is_ok(),
        to_decide: brief.claims_to_decide(sheet),
        claims: brief.claims().len(),
        script_outdated: script
            .is_some_and(|script| brief.outdates_script(script.source().generation().generated_at)),
    }
}

fn now() -> SystemTime {
    crate::publications::whole_millis(SystemTime::now())
}

impl Bardo {
    fn brief_project(&self, id: VideoProjectId) -> Result<VideoProject, BriefError> {
        self.themes
            .project(id)?
            .filter(|project| project.owner == self.profile.id)
            .ok_or(BriefError::ProjectNotFound)
    }

    /// The project's brief and its product's sheet as it is now. A
    /// product whose sheet is gone reads as one that confirms nothing.
    fn brief_to_change(
        &self,
        project: VideoProjectId,
    ) -> Result<(AdBrief, ProductSheet), BriefError> {
        let project = self.brief_project(project)?;
        let brief = self.ads.brief(project.id)?.ok_or(BriefError::NotAnAd)?;
        let sheet = self.sheet_or_empty(brief.product)?;
        Ok((brief, sheet))
    }

    fn sheet_or_empty(&self, product: ProductId) -> Result<ProductSheet, RepositoryError> {
        Ok(self
            .products
            .sheet(product)?
            .unwrap_or_else(|| ProductSheet::empty(product, now())))
    }

    /// Where the brief of `project` stands; `None` when it is no ad.
    pub(crate) fn brief_standing(
        &self,
        project: &VideoProject,
        script: Option<&Script>,
    ) -> Result<Option<BriefStanding>, RepositoryError> {
        if !project.is_ad() {
            return Ok(None);
        }
        let Some(brief) = self.ads.brief(project.id)? else {
            return Ok(None);
        };
        let sheet = self.sheet_or_empty(brief.product)?;
        Ok(Some(standing(&brief, &sheet, script)))
    }

    /// The project's Brief stage; `None` for a project that is no ad.
    pub fn ad_brief(&self, project: VideoProjectId) -> Result<Option<BriefView>, BriefError> {
        let project = self.brief_project(project)?;
        let Some(brief) = self.ads.brief(project.id)? else {
            return Ok(None);
        };
        let product = self
            .products
            .get(brief.product)?
            .filter(|product| product.owner == self.profile.id)
            .ok_or(BriefError::ProjectNotFound)?;
        let sheet = self.sheet_or_empty(product.id)?;
        let angle = self.ads.angle(brief.angle)?;
        let leans_on: &[ClaimId] = angle.as_ref().map_or(&[], |angle| angle.idea().claims());
        let claims = brief
            .claims()
            .iter()
            .map(|copy| BriefClaimRow {
                drift: copy.drift(&sheet),
                in_angle: leans_on.contains(&copy.claim),
                copy: copy.clone(),
            })
            .collect();
        let available = sheet
            .confirmed()
            .filter(|claim| !brief.uses(claim.id))
            .cloned()
            .collect();
        let script = self.scripts.script(project.id)?;
        Ok(Some(BriefView {
            standing: standing(&brief, &sheet, script.as_ref()),
            not_ready: brief.readiness(&sheet).err(),
            claims,
            available,
            angle,
            product,
            project,
            brief,
        }))
    }

    /// Saves the offer, the call to action and the link of this video.
    pub fn save_brief_text(
        &self,
        project: VideoProjectId,
        offer: &str,
        call_to_action: &str,
        link: &str,
    ) -> Result<AdBrief, BriefError> {
        let (mut brief, _) = self.brief_to_change(project)?;
        let text = BriefText::new(offer, call_to_action, link).map_err(BriefError::Invalid)?;
        if brief.set_text(text, now()) {
            self.ads.save_brief(&brief)?;
        }
        Ok(brief)
    }

    /// Switches a claim of the product on or off for this video. Switched
    /// on, the video takes a copy of the claim as the product states it
    /// now.
    pub fn set_brief_claim(
        &self,
        project: VideoProjectId,
        claim: ClaimId,
        on: bool,
    ) -> Result<AdBrief, BriefError> {
        let (mut brief, sheet) = self.brief_to_change(project)?;
        if on {
            let claim = sheet.claim(claim).ok_or(BriefClaimError::NotConfirmed)?;
            brief.use_claim(claim, now())?;
        } else {
            brief.drop_claim(claim, now())?;
        }
        self.ads.save_brief(&brief)?;
        Ok(brief)
    }

    /// Keeps the video's copy of a claim over what the product says now.
    pub fn keep_brief_claim(
        &self,
        project: VideoProjectId,
        claim: ClaimId,
    ) -> Result<AdBrief, BriefError> {
        let (mut brief, sheet) = self.brief_to_change(project)?;
        brief.keep_claim(claim, &sheet)?;
        self.ads.save_brief(&brief)?;
        Ok(brief)
    }

    /// Takes the product's claim as it reads now in place of the video's
    /// copy. A claim the product no longer confirms cannot be taken: drop
    /// it instead.
    pub fn update_brief_claim(
        &self,
        project: VideoProjectId,
        claim: ClaimId,
    ) -> Result<AdBrief, BriefError> {
        let (mut brief, sheet) = self.brief_to_change(project)?;
        let current = sheet.claim(claim).ok_or(BriefClaimError::NotConfirmed)?;
        if current.state() != bardo_domain::ClaimState::Confirmed {
            return Err(BriefClaimError::NotConfirmed.into());
        }
        let now = now();
        brief.drop_claim(claim, now)?;
        brief.use_claim(current, now)?;
        self.ads.save_brief(&brief)?;
        Ok(brief)
    }

    /// Confirms the brief: the Script stage opens.
    pub fn confirm_brief(&self, project: VideoProjectId) -> Result<AdBrief, BriefError> {
        let (mut brief, sheet) = self.brief_to_change(project)?;
        brief.confirm(&sheet, now()).map_err(BriefError::NotReady)?;
        self.ads.save_brief(&brief)?;
        Ok(brief)
    }

    /// The user read the script against the changed brief and keeps it.
    pub fn keep_script_after_brief(&self, project: VideoProjectId) -> Result<AdBrief, BriefError> {
        let (mut brief, _) = self.brief_to_change(project)?;
        brief.keep_script(now());
        self.ads.save_brief(&brief)?;
        Ok(brief)
    }

    /// After a claim of `product` changed: lets every brief that copied
    /// it forget a copy kept over something the product no longer says
    /// otherwise, so that a later change there is asked about anew.
    /// Whether a brief shows Attention is read from the sheet each time;
    /// nothing else is stored.
    pub(crate) fn settle_briefs(&self, product: ProductId, sheet: &ProductSheet) {
        let briefs = match self.ads.briefs_of(product) {
            Ok(briefs) => briefs,
            Err(error) => {
                tracing::warn!("could not read the briefs of a product: {error}");
                return;
            }
        };
        for mut brief in briefs {
            if brief.settle(sheet)
                && let Err(error) = self.ads.save_brief(&brief)
            {
                tracing::warn!("could not settle a brief: {error}");
            }
        }
    }

    /// An ad project's length changed: its script reads it.
    pub(crate) fn touch_brief(&self, project: VideoProjectId) -> Result<(), RepositoryError> {
        if let Some(mut brief) = self.ads.brief(project)? {
            brief.touch(now());
            self.ads.save_brief(&brief)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bardo_domain::{ClaimKind, VideoLength};

    use super::*;
    use crate::ad_angles::testing::{ad_project, claim_id, start};
    use crate::scenes::tests::{Harness, SCRIPT, done};
    use crate::stages::{Stage, StageNote, StageState, StageStatus, opening_stage, project_stages};
    use crate::{BudgetConsent, ScriptError};

    fn stages(app: &Bardo, project: &VideoProject) -> Vec<StageStatus> {
        project_stages(
            &app.script(project.id).unwrap(),
            &app.narration(project.id).unwrap(),
            &app.scenes(project.id).unwrap(),
            &app.render_summary(project.id).unwrap(),
            &app.export_summary(project.id).unwrap(),
        )
    }

    fn stage(app: &Bardo, project: &VideoProject, stage: Stage) -> StageStatus {
        stages(app, project)
            .into_iter()
            .find(|status| status.stage == stage)
            .unwrap_or_else(|| panic!("no {stage:?} stage"))
    }

    fn brief(app: &Bardo, project: &VideoProject) -> BriefView {
        app.ad_brief(project.id).unwrap().unwrap()
    }

    fn texts(view: &BriefView) -> Vec<&str> {
        view.claims
            .iter()
            .map(|row| row.copy.text.as_str())
            .collect()
    }

    /// Later than anything saved so far, as the store keeps milliseconds.
    fn tick() {
        std::thread::sleep(Duration::from_millis(3));
    }

    #[test]
    fn an_ad_project_opens_on_its_brief_with_the_script_locked_until_confirmed() {
        let h = Harness::new();
        let app = start(&h);
        let (product, _, project) = ad_project(&h, &app);

        let all = stages(&app, &project);
        assert_eq!(all[0].stage, Stage::Brief);
        assert_eq!(
            (all[0].state, all[0].note),
            (StageState::Open, StageNote::BriefToConfirm)
        );
        assert_eq!(
            (all[1].stage, all[1].state, all[1].note),
            (
                Stage::Script,
                StageState::Locked,
                StageNote::After(Stage::Brief)
            )
        );
        assert_eq!(opening_stage(&all), Stage::Brief);
        assert!(matches!(
            app.generate_script(project.id, BudgetConsent::Ask),
            Err(ScriptError::BriefNotConfirmed)
        ));

        let view = brief(&app, &project);
        assert_eq!(view.product.id, product.id);
        assert_eq!(
            texts(&view),
            ["Refillable pages", "A5 size", "Lasts 3 school years"]
        );
        // The approved angle leans on claims 1 and 3.
        let in_angle: Vec<bool> = view.claims.iter().map(|row| row.in_angle).collect();
        assert_eq!(in_angle, [true, false, true]);
        assert!(view.available.is_empty());
        assert_eq!(view.brief.text().offer(), "20% off until Friday");
        assert_eq!(view.brief.text().link(), "https://example.com/notebook");
        assert_eq!(view.not_ready, None);
        assert_eq!(project.length, Some(VideoLength::Seconds30));

        app.confirm_brief(project.id).unwrap();
        let all = stages(&app, &project);
        assert_eq!(
            (all[0].state, all[0].note),
            (StageState::Done, StageNote::BriefClaims(3))
        );
        assert_eq!(
            (all[1].state, all[1].note),
            (StageState::Open, StageNote::ScriptMissing)
        );
        h.answer(SCRIPT.to_owned());
        done(
            &app,
            app.generate_script(project.id, BudgetConsent::Ask).unwrap(),
        );
        assert_eq!(stage(&app, &project, Stage::Script).state, StageState::Done);
    }

    #[test]
    fn a_theme_project_has_no_brief_and_its_stages_are_as_before() {
        let h = Harness::new();
        let app = start(&h);
        let project = crate::scenes::tests::project(&app);
        assert_eq!(app.ad_brief(project.id).unwrap(), None);
        assert_eq!(app.script(project.id).unwrap().brief, None);
        let all = stages(&app, &project);
        assert_eq!(all[0].stage, Stage::Script);
        assert!(all.iter().all(|status| status.stage != Stage::Brief));
        assert!(matches!(
            app.confirm_brief(project.id),
            Err(BriefError::NotAnAd)
        ));
    }

    #[test]
    fn a_claim_is_switched_off_for_this_video_and_on_again() {
        let h = Harness::new();
        let app = start(&h);
        let (product, _, project) = ad_project(&h, &app);
        let a5 = claim_id(&app, product.id, "A5 size");

        app.set_brief_claim(project.id, a5, false).unwrap();
        let view = brief(&app, &project);
        assert_eq!(texts(&view), ["Refillable pages", "Lasts 3 school years"]);
        assert_eq!(view.available.len(), 1);
        assert_eq!(view.available[0].id, a5);
        // The product's sheet is untouched.
        assert_eq!(
            app.products
                .sheet(product.id)
                .unwrap()
                .unwrap()
                .claims()
                .len(),
            3
        );

        app.set_brief_claim(project.id, a5, true).unwrap();
        assert_eq!(brief(&app, &project).claims.len(), 3);
        assert!(matches!(
            app.set_brief_claim(project.id, ClaimId::new(), true),
            Err(BriefError::Claim(BriefClaimError::NotConfirmed))
        ));
        assert!(matches!(
            app.set_brief_claim(project.id, ClaimId::new(), false),
            Err(BriefError::Claim(BriefClaimError::NotUsed))
        ));
    }

    #[test]
    fn the_brief_needs_a_claim_and_a_call_to_action_and_checks_its_text() {
        let h = Harness::new();
        let app = start(&h);
        let (_, _, project) = ad_project(&h, &app);

        let error = app
            .save_brief_text(project.id, &"x".repeat(301), "Get yours", "")
            .unwrap_err();
        assert_eq!(error.field_errors(), [BriefFieldError::OfferTooLong]);

        app.save_brief_text(project.id, "Free shipping", " ", "@notebook")
            .unwrap();
        let view = brief(&app, &project);
        assert_eq!(view.brief.text().offer(), "Free shipping");
        assert_eq!(view.not_ready, Some(BriefNotReady::NoCallToAction));
        let error = app.confirm_brief(project.id).unwrap_err();
        assert!(matches!(
            error,
            BriefError::NotReady(BriefNotReady::NoCallToAction)
        ));
        assert_eq!(
            error.message(),
            Some(Text::BriefNotReady(BriefNotReady::NoCallToAction))
        );

        app.save_brief_text(project.id, "Free shipping", "Order today", "@notebook")
            .unwrap();
        for row in brief(&app, &project).claims {
            app.set_brief_claim(project.id, row.copy.claim, false)
                .unwrap();
        }
        assert!(matches!(
            app.confirm_brief(project.id),
            Err(BriefError::NotReady(BriefNotReady::NoClaim))
        ));
    }

    #[test]
    fn a_claim_edited_or_rejected_in_the_product_puts_every_brief_that_copied_it_in_attention() {
        let h = Harness::new();
        let app = start(&h);
        let (product, channel, project) = ad_project(&h, &app);
        // A second video of the same product.
        let angles = app.ad_angles(product.id, Some(channel.id)).unwrap();
        let other = app
            .approve_ad_angle(
                angles
                    .angles
                    .iter()
                    .find(|row| row.project.is_none())
                    .unwrap()
                    .angle
                    .id,
            )
            .unwrap();
        app.confirm_brief(project.id).unwrap();
        let a5 = claim_id(&app, product.id, "A5 size");
        let pages = claim_id(&app, product.id, "Refillable pages");

        app.edit_claim(product.id, a5, "A4 size").unwrap();
        app.reject_claim(product.id, pages).unwrap();

        for project in [&project, &other] {
            let status = stage(&app, project, Stage::Brief);
            assert_eq!(
                (status.state, status.note),
                (StageState::Attention, StageNote::BriefClaimsChanged(2))
            );
            let view = brief(&app, project);
            // The video still says what it said.
            assert_eq!(
                texts(&view),
                ["Refillable pages", "A5 size", "Lasts 3 school years"]
            );
            let drift: Vec<_> = view.claims.iter().map(|row| row.drift.clone()).collect();
            assert_eq!(
                drift,
                [
                    Some(ClaimDrift::Unconfirmed),
                    Some(ClaimDrift::Edited {
                        now: "A4 size".into()
                    }),
                    None,
                ]
            );
            assert_eq!(view.not_ready, Some(BriefNotReady::ClaimsToDecide));
        }
        assert!(matches!(
            app.confirm_brief(other.id),
            Err(BriefError::NotReady(BriefNotReady::ClaimsToDecide))
        ));

        // Keeping one copy and dropping the other clears it.
        app.keep_brief_claim(project.id, a5).unwrap();
        assert_eq!(
            stage(&app, &project, Stage::Brief).note,
            StageNote::BriefClaimsChanged(1)
        );
        app.set_brief_claim(project.id, pages, false).unwrap();
        let status = stage(&app, &project, Stage::Brief);
        assert_eq!(
            (status.state, status.note),
            (StageState::Done, StageNote::BriefClaims(2))
        );
        assert_eq!(
            texts(&brief(&app, &project)),
            ["A5 size", "Lasts 3 school years"]
        );
        // The other video still waits for its own answer.
        assert_eq!(
            stage(&app, &other, Stage::Brief).state,
            StageState::Attention
        );

        // Taking the product's new words is the other answer.
        app.update_brief_claim(other.id, a5).unwrap();
        assert!(texts(&brief(&app, &other)).contains(&"A4 size"));
        assert!(matches!(
            app.update_brief_claim(other.id, pages),
            Err(BriefError::Claim(BriefClaimError::NotConfirmed))
        ));
    }

    #[test]
    fn a_kept_copy_is_asked_about_again_only_after_the_product_changes_again() {
        let h = Harness::new();
        let app = start(&h);
        let (product, _, project) = ad_project(&h, &app);
        let a5 = claim_id(&app, product.id, "A5 size");

        app.edit_claim(product.id, a5, "A4 size").unwrap();
        app.keep_brief_claim(project.id, a5).unwrap();
        assert_eq!(brief(&app, &project).standing.to_decide, 0);

        // The same edit saved again asks nothing; another one does.
        app.set_claim_kind(product.id, a5, ClaimKind::Proof)
            .unwrap();
        assert_eq!(brief(&app, &project).standing.to_decide, 0);
        app.edit_claim(product.id, a5, "A3 size").unwrap();
        assert_eq!(brief(&app, &project).standing.to_decide, 1);

        // Back to what the video says: nothing to decide, and what was
        // kept is forgotten, so the next change asks again.
        app.edit_claim(product.id, a5, "A5 size").unwrap();
        let view = brief(&app, &project);
        assert_eq!(view.standing.to_decide, 0);
        assert!(view.claims.iter().all(|row| row.copy.kept.is_none()));
        app.edit_claim(product.id, a5, "A4 size").unwrap();
        assert_eq!(brief(&app, &project).standing.to_decide, 1);
    }

    #[test]
    fn a_change_after_the_script_marks_it_until_it_is_kept_or_written_again() {
        let h = Harness::new();
        let app = start(&h);
        let (product, _, project) = ad_project(&h, &app);
        app.confirm_brief(project.id).unwrap();
        h.answer(SCRIPT.to_owned());
        done(
            &app,
            app.generate_script(project.id, BudgetConsent::Ask).unwrap(),
        );
        assert_eq!(stage(&app, &project, Stage::Script).state, StageState::Done);
        assert!(!brief(&app, &project).standing.script_outdated);

        // Saving the same words changes nothing.
        tick();
        app.save_brief_text(
            project.id,
            "20% off until Friday",
            "Get yours",
            "https://example.com/notebook",
        )
        .unwrap();
        assert_eq!(stage(&app, &project, Stage::Script).state, StageState::Done);

        app.save_brief_text(project.id, "30% off", "Get yours", "example.com/notebook")
            .unwrap();
        let status = stage(&app, &project, Stage::Script);
        assert_eq!(
            (status.state, status.note),
            (StageState::Attention, StageNote::ScriptBriefChanged)
        );
        assert!(brief(&app, &project).standing.script_outdated);

        tick();
        app.keep_script_after_brief(project.id).unwrap();
        assert_eq!(stage(&app, &project, Stage::Script).state, StageState::Done);

        // A claim switched off and the length are read by a script too.
        tick();
        let a5 = claim_id(&app, product.id, "A5 size");
        app.set_brief_claim(project.id, a5, false).unwrap();
        assert_eq!(
            stage(&app, &project, Stage::Script).note,
            StageNote::ScriptBriefChanged
        );
        tick();
        app.keep_script_after_brief(project.id).unwrap();
        tick();
        app.set_project_length(project.id, Some(VideoLength::Minute1))
            .unwrap();
        assert_eq!(
            stage(&app, &project, Stage::Script).note,
            StageNote::ScriptBriefChanged
        );
        // The same length again is no change.
        tick();
        app.keep_script_after_brief(project.id).unwrap();
        tick();
        app.set_project_length(project.id, Some(VideoLength::Minute1))
            .unwrap();
        assert_eq!(stage(&app, &project, Stage::Script).state, StageState::Done);

        // A script written again reads the brief as it is.
        tick();
        app.save_brief_text(project.id, "40% off", "Get yours", "")
            .unwrap();
        h.answer(SCRIPT.to_owned());
        done(
            &app,
            app.generate_script(project.id, BudgetConsent::Ask).unwrap(),
        );
        assert_eq!(
            stage(&app, &project, Stage::Script).note,
            StageNote::ScriptToReview
        );
        app.accept_script(project.id).unwrap();
        assert_eq!(stage(&app, &project, Stage::Script).state, StageState::Done);
    }

    #[test]
    fn keeping_a_copy_marks_no_script() {
        let h = Harness::new();
        let app = start(&h);
        let (product, _, project) = ad_project(&h, &app);
        app.confirm_brief(project.id).unwrap();
        h.answer(SCRIPT.to_owned());
        done(
            &app,
            app.generate_script(project.id, BudgetConsent::Ask).unwrap(),
        );
        tick();
        let a5 = claim_id(&app, product.id, "A5 size");
        app.edit_claim(product.id, a5, "A4 size").unwrap();
        app.keep_brief_claim(project.id, a5).unwrap();
        assert_eq!(stage(&app, &project, Stage::Script).state, StageState::Done);
    }

    #[test]
    fn an_ad_project_refuses_a_version_in_another_language() {
        use crate::localization::{LocalizationError, VersionBlock};
        use bardo_domain::{ChannelDraft, ContentLanguage, Country};

        let h = Harness::new();
        let app = start(&h);
        let (_, _, project) = ad_project(&h, &app);
        let other = app
            .create_channel(ChannelDraft {
                name: "Mesa de Estudos".into(),
                niche: "dicas de estudo".into(),
                language: ContentLanguage::Portuguese,
                country: Country::Brazil,
                ..ChannelDraft::default()
            })
            .unwrap();

        let versions = app.versions(project.id).unwrap();
        assert_eq!(versions.blocked, Some(VersionBlock::Ad));
        assert!(versions.targets.is_empty());
        let error = app
            .make_version(project.id, other.id, BudgetConsent::Ask)
            .unwrap_err();
        assert!(matches!(error, LocalizationError::AdProject));
        assert_eq!(error.message(), Text::VersionAdProject);
    }

    #[test]
    fn other_profiles_cannot_reach_a_brief() {
        let h = Harness::new();
        let app = start(&h);
        assert!(matches!(
            app.ad_brief(VideoProjectId::new()),
            Err(BriefError::ProjectNotFound)
        ));
        assert!(matches!(
            app.confirm_brief(VideoProjectId::new()),
            Err(BriefError::ProjectNotFound)
        ));
    }
}
