//! The brief of an ad project (docs/spec/marketing-videos.md, CONTEXT.md):
//! what this one video says about its product. The Brief stage is the
//! first stage of an ad project and of no other.
//!
//! A brief holds the video's own copy of the claims it uses, so editing a
//! product never changes a video silently. When the product's claim is
//! later edited or no longer confirmed, the copy stays as it was and the
//! brief says so, until the user keeps the copy or drops it.

use std::time::SystemTime;

use crate::{
    AdAngleId, Claim, ClaimId, ClaimKind, ClaimState, Product, ProductBrief, ProductId,
    ProductSheet, VideoProjectId,
};

/// What a product's claim said at one moment: its text, and whether the
/// user stood behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimSeen {
    pub text: String,
    pub confirmed: bool,
}

impl ClaimSeen {
    /// What the sheet says of the claim now. A claim the sheet no longer
    /// holds is not confirmed.
    fn of(claim: Option<&Claim>) -> Self {
        match claim {
            Some(claim) => Self {
                text: claim.text().to_owned(),
                confirmed: claim.state() == ClaimState::Confirmed,
            },
            None => Self {
                text: String::new(),
                confirmed: false,
            },
        }
    }
}

/// How a product's claim moved away from a video's copy of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimDrift {
    /// The product's claim now reads `now`.
    Edited { now: String },
    /// The product no longer confirms the claim.
    Unconfirmed,
}

/// A video's own copy of a claim it uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BriefClaim {
    /// The product's claim it was copied from.
    pub claim: ClaimId,
    /// The claim as the video states it.
    pub text: String,
    pub kind: ClaimKind,
    /// Whether the claim was sensitive when copied: the video's reviews
    /// list it again.
    pub sensitive: bool,
    /// What the product's claim said when the user chose to keep this
    /// copy over it; the same change is not asked about twice.
    pub kept: Option<ClaimSeen>,
}

impl BriefClaim {
    fn copy_of(claim: &Claim) -> Self {
        Self {
            claim: claim.id,
            text: claim.text().to_owned(),
            kind: claim.kind,
            sensitive: claim.sensitive(),
            kept: None,
        }
    }

    /// Whether what the product says now is what the copy says.
    fn agrees(&self, now: &ClaimSeen) -> bool {
        now.confirmed && now.text == self.text
    }

    /// How the product's claim moved away from this copy, unless the user
    /// already kept the copy over that very change.
    pub fn drift(&self, sheet: &ProductSheet) -> Option<ClaimDrift> {
        let now = ClaimSeen::of(sheet.claim(self.claim));
        if self.agrees(&now) || self.kept.as_ref() == Some(&now) {
            return None;
        }
        Some(if now.confirmed {
            ClaimDrift::Edited { now: now.text }
        } else {
            ClaimDrift::Unconfirmed
        })
    }
}

/// Why typed brief text is not valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BriefFieldError {
    OfferTooLong,
    CallToActionTooLong,
    LinkTooLong,
}

impl BriefFieldError {
    pub const ALL: [BriefFieldError; 3] = [
        BriefFieldError::OfferTooLong,
        BriefFieldError::CallToActionTooLong,
        BriefFieldError::LinkTooLong,
    ];
}

/// What the video offers and asks for, always valid: each on one line and
/// within the product brief's limits. They start as the product's and are
/// this video's from then on.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BriefText {
    offer: String,
    call_to_action: String,
    link: String,
}

impl BriefText {
    pub fn new(
        offer: &str,
        call_to_action: &str,
        link: &str,
    ) -> Result<Self, Vec<BriefFieldError>> {
        let line = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
        let too_long = |text: &str, max: usize| text.chars().count() > max;
        let offer = line(offer);
        let call_to_action = line(call_to_action);
        let link = line(link);
        let mut errors = Vec::new();
        if too_long(&offer, ProductBrief::MAX_OFFER_CHARS) {
            errors.push(BriefFieldError::OfferTooLong);
        }
        if too_long(&call_to_action, ProductBrief::MAX_CALL_TO_ACTION_CHARS) {
            errors.push(BriefFieldError::CallToActionTooLong);
        }
        if too_long(&link, ProductBrief::MAX_LINK_CHARS) {
            errors.push(BriefFieldError::LinkTooLong);
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(Self {
            offer,
            call_to_action,
            link,
        })
    }

    /// What the viewer gets; may be empty.
    pub fn offer(&self) -> &str {
        &self.offer
    }

    /// What the viewer is asked to do, in the user's words.
    pub fn call_to_action(&self) -> &str {
        &self.call_to_action
    }

    /// Where to go: a web address or a handle; may be empty.
    pub fn link(&self) -> &str {
        &self.link
    }
}

/// Why a brief cannot be confirmed yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum BriefNotReady {
    /// A script may state only the brief's claims, so it needs one.
    #[error("the brief uses no claim")]
    NoClaim,
    /// A claim changed in the product and the user has not said whether
    /// the video keeps its copy.
    #[error("a claim changed in the product")]
    ClaimsToDecide,
    /// The call to action is the user's own words.
    #[error("the brief has no call to action")]
    NoCallToAction,
}

impl BriefNotReady {
    pub const ALL: [BriefNotReady; 3] = [
        BriefNotReady::NoClaim,
        BriefNotReady::ClaimsToDecide,
        BriefNotReady::NoCallToAction,
    ];
}

/// Why a claim cannot be used in a brief.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BriefClaimError {
    /// Only a claim the user confirmed reaches an ad.
    #[error("the product does not confirm the claim")]
    NotConfirmed,
    #[error("the brief does not use the claim")]
    NotUsed,
}

/// What one ad project says about its product.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdBrief {
    pub project: VideoProjectId,
    pub product: ProductId,
    /// The ad angle the project started from.
    pub angle: AdAngleId,
    text: BriefText,
    claims: Vec<BriefClaim>,
    confirmed_at: Option<SystemTime>,
    /// When what a script reads here last changed.
    changed_at: SystemTime,
    /// When the user last said the script still holds after a change.
    script_kept_at: Option<SystemTime>,
}

/// Every stored field of a brief, for adapters that rebuild one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdBriefRecord {
    pub project: VideoProjectId,
    pub product: ProductId,
    pub angle: AdAngleId,
    pub text: BriefText,
    pub claims: Vec<BriefClaim>,
    pub confirmed_at: Option<SystemTime>,
    pub changed_at: SystemTime,
    pub script_kept_at: Option<SystemTime>,
}

impl AdBrief {
    /// The brief a new ad project starts with: a copy of every claim the
    /// product confirms, and the product's own offer, call to action and
    /// link. It waits for the user to confirm it.
    pub fn start(
        project: VideoProjectId,
        product: &Product,
        angle: AdAngleId,
        sheet: &ProductSheet,
        now: SystemTime,
    ) -> Self {
        let brief = &product.brief;
        Self {
            project,
            product: product.id,
            angle,
            // The product's own text is within the same limits.
            text: BriefText::new(brief.offer(), brief.call_to_action(), brief.link())
                .unwrap_or_default(),
            claims: sheet.confirmed().map(BriefClaim::copy_of).collect(),
            confirmed_at: None,
            changed_at: now,
            script_kept_at: None,
        }
    }

    pub fn restore(record: AdBriefRecord) -> Self {
        Self {
            project: record.project,
            product: record.product,
            angle: record.angle,
            text: record.text,
            claims: record.claims,
            confirmed_at: record.confirmed_at,
            changed_at: record.changed_at,
            script_kept_at: record.script_kept_at,
        }
    }

    pub fn text(&self) -> &BriefText {
        &self.text
    }

    /// The claims the video uses, as it states them.
    pub fn claims(&self) -> &[BriefClaim] {
        &self.claims
    }

    pub fn uses(&self, claim: ClaimId) -> bool {
        self.claims.iter().any(|copy| copy.claim == claim)
    }

    pub fn confirmed_at(&self) -> Option<SystemTime> {
        self.confirmed_at
    }

    /// Whether the user confirmed the brief: the Script stage opens then.
    pub fn is_confirmed(&self) -> bool {
        self.confirmed_at.is_some()
    }

    pub fn changed_at(&self) -> SystemTime {
        self.changed_at
    }

    pub fn script_kept_at(&self) -> Option<SystemTime> {
        self.script_kept_at
    }

    /// How many copies wait for the user to keep or drop them, given what
    /// the product says now.
    pub fn claims_to_decide(&self, sheet: &ProductSheet) -> usize {
        self.claims
            .iter()
            .filter(|copy| copy.drift(sheet).is_some())
            .count()
    }

    /// What stands in the way of confirming the brief, the first of them.
    pub fn readiness(&self, sheet: &ProductSheet) -> Result<(), BriefNotReady> {
        if self.claims_to_decide(sheet) > 0 {
            return Err(BriefNotReady::ClaimsToDecide);
        }
        if self.claims.is_empty() {
            return Err(BriefNotReady::NoClaim);
        }
        if self.text.call_to_action.is_empty() {
            return Err(BriefNotReady::NoCallToAction);
        }
        Ok(())
    }

    /// The user confirms the brief. Confirming one already confirmed
    /// changes nothing.
    pub fn confirm(&mut self, sheet: &ProductSheet, now: SystemTime) -> Result<(), BriefNotReady> {
        self.readiness(sheet)?;
        if self.confirmed_at.is_none() {
            self.confirmed_at = Some(now);
        }
        Ok(())
    }

    /// Replaces the offer, the call to action and the link. Returns
    /// whether anything changed.
    pub fn set_text(&mut self, text: BriefText, now: SystemTime) -> bool {
        if text == self.text {
            return false;
        }
        self.text = text;
        self.changed_at = now;
        true
    }

    /// Takes a claim of the product into the video, as it reads now. One
    /// already used stays as the video has it.
    pub fn use_claim(&mut self, claim: &Claim, now: SystemTime) -> Result<(), BriefClaimError> {
        if self.uses(claim.id) {
            return Ok(());
        }
        if claim.state() != ClaimState::Confirmed {
            return Err(BriefClaimError::NotConfirmed);
        }
        self.claims.push(BriefClaim::copy_of(claim));
        self.changed_at = now;
        Ok(())
    }

    /// Takes a claim out of the video: one the user switched off, or a
    /// copy dropped after its claim changed in the product.
    pub fn drop_claim(&mut self, claim: ClaimId, now: SystemTime) -> Result<(), BriefClaimError> {
        let at = self
            .claims
            .iter()
            .position(|copy| copy.claim == claim)
            .ok_or(BriefClaimError::NotUsed)?;
        self.claims.remove(at);
        self.changed_at = now;
        Ok(())
    }

    /// The user keeps the video's copy over what the product says now.
    /// The video's text does not change, so nothing made from it is
    /// marked.
    pub fn keep_claim(
        &mut self,
        claim: ClaimId,
        sheet: &ProductSheet,
    ) -> Result<(), BriefClaimError> {
        let copy = self
            .claims
            .iter_mut()
            .find(|copy| copy.claim == claim)
            .ok_or(BriefClaimError::NotUsed)?;
        let now = ClaimSeen::of(sheet.claim(claim));
        copy.kept = if copy.agrees(&now) { None } else { Some(now) };
        Ok(())
    }

    /// Forgets what the user kept a copy over once the product says what
    /// the copy says again, so a later change there is asked about anew.
    /// Returns whether anything was forgotten.
    pub fn settle(&mut self, sheet: &ProductSheet) -> bool {
        let mut settled = false;
        for copy in &mut self.claims {
            if copy.kept.is_some() && copy.agrees(&ClaimSeen::of(sheet.claim(copy.claim))) {
                copy.kept = None;
                settled = true;
            }
        }
        settled
    }

    /// Something a script reads outside the brief changed (the video's
    /// length).
    pub fn touch(&mut self, now: SystemTime) {
        self.changed_at = now;
    }

    /// Whether a script written at `written_at` read an earlier brief and
    /// the user has not said since that it still holds.
    pub fn outdates_script(&self, written_at: SystemTime) -> bool {
        self.changed_at > written_at
            && self
                .script_kept_at
                .is_none_or(|kept| self.changed_at > kept)
    }

    /// The user read the script against the changed brief and keeps it.
    pub fn keep_script(&mut self, now: SystemTime) {
        self.script_kept_at = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::{ProductDraft, ProfileId};

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn product() -> Product {
        let brief = ProductBrief::validate(ProductDraft {
            name: "Lumen lamp".into(),
            description: "A desk lamp that folds flat.".into(),
            offer: "Free shipping this month".into(),
            call_to_action: "Order yours today".into(),
            link: "lumen.example/lamp".into(),
            ..ProductDraft::default()
        })
        .unwrap();
        Product::new(ProfileId::new(), brief)
    }

    /// A sheet with two confirmed claims, one found and one rejected.
    fn sheet(product: &Product) -> (ProductSheet, ClaimId, ClaimId) {
        let mut sheet = ProductSheet::empty(product.id, at(1));
        let folds = sheet
            .add_claim("Folds flat", ClaimKind::Feature, at(1))
            .unwrap();
        let lasts = sheet
            .add_claim("Lasts 40 hours on a charge", ClaimKind::Proof, at(1))
            .unwrap();
        let gone = sheet
            .add_claim("Made of steel", ClaimKind::Feature, at(1))
            .unwrap();
        sheet.change_claim(gone, at(2), Claim::reject).unwrap();
        (sheet, folds, lasts)
    }

    fn brief(product: &Product, sheet: &ProductSheet) -> AdBrief {
        AdBrief::start(
            VideoProjectId::new(),
            product,
            AdAngleId::new(),
            sheet,
            at(10),
        )
    }

    fn texts(brief: &AdBrief) -> Vec<&str> {
        brief
            .claims()
            .iter()
            .map(|copy| copy.text.as_str())
            .collect()
    }

    #[test]
    fn a_brief_starts_with_the_confirmed_claims_and_the_products_own_words() {
        let product = product();
        let (sheet, folds, lasts) = sheet(&product);
        let brief = brief(&product, &sheet);

        assert_eq!(texts(&brief), ["Folds flat", "Lasts 40 hours on a charge"]);
        assert!(brief.uses(folds) && brief.uses(lasts));
        assert!(!brief.claims()[0].sensitive);
        assert!(brief.claims()[1].sensitive, "a claim with a number");
        assert_eq!(brief.claims()[1].kind, ClaimKind::Proof);
        assert_eq!(brief.text().offer(), "Free shipping this month");
        assert_eq!(brief.text().call_to_action(), "Order yours today");
        assert_eq!(brief.text().link(), "https://lumen.example/lamp");
        assert!(!brief.is_confirmed());
        assert_eq!(brief.claims_to_decide(&sheet), 0);
    }

    #[test]
    fn brief_text_is_one_line_each_and_within_the_products_limits() {
        let text = BriefText::new("  Two for\none  ", " Get it\n now ", " @lumen ").unwrap();
        assert_eq!(text.offer(), "Two for one");
        assert_eq!(text.call_to_action(), "Get it now");
        assert_eq!(text.link(), "@lumen");

        assert!(
            BriefText::new(
                &"é".repeat(ProductBrief::MAX_OFFER_CHARS),
                &"é".repeat(ProductBrief::MAX_CALL_TO_ACTION_CHARS),
                &"é".repeat(ProductBrief::MAX_LINK_CHARS),
            )
            .is_ok()
        );
        assert_eq!(
            BriefText::new(
                &"a".repeat(ProductBrief::MAX_OFFER_CHARS + 1),
                &"a".repeat(ProductBrief::MAX_CALL_TO_ACTION_CHARS + 1),
                &"a".repeat(ProductBrief::MAX_LINK_CHARS + 1),
            ),
            Err(BriefFieldError::ALL.to_vec())
        );
    }

    #[test]
    fn confirming_needs_a_claim_and_a_call_to_action() {
        let product = product();
        let (sheet, folds, lasts) = sheet(&product);
        let mut brief = brief(&product, &sheet);

        brief.set_text(BriefText::new("Free shipping", "", "").unwrap(), at(11));
        assert_eq!(
            brief.confirm(&sheet, at(12)),
            Err(BriefNotReady::NoCallToAction)
        );
        brief.set_text(BriefText::new("", "Order today", "").unwrap(), at(13));

        brief.drop_claim(folds, at(14)).unwrap();
        brief.drop_claim(lasts, at(14)).unwrap();
        assert_eq!(brief.confirm(&sheet, at(15)), Err(BriefNotReady::NoClaim));
        assert!(!brief.is_confirmed());

        brief
            .use_claim(sheet.claim(folds).unwrap(), at(16))
            .unwrap();
        brief.confirm(&sheet, at(17)).unwrap();
        assert_eq!(brief.confirmed_at(), Some(at(17)));
        // Confirming again keeps the first time.
        brief.confirm(&sheet, at(99)).unwrap();
        assert_eq!(brief.confirmed_at(), Some(at(17)));
    }

    #[test]
    fn a_claim_is_switched_off_and_on_again_as_the_product_states_it_now() {
        let product = product();
        let (mut sheet, folds, _) = sheet(&product);
        let mut brief = brief(&product, &sheet);

        brief.drop_claim(folds, at(11)).unwrap();
        assert_eq!(texts(&brief), ["Lasts 40 hours on a charge"]);
        assert_eq!(
            brief.drop_claim(folds, at(11)),
            Err(BriefClaimError::NotUsed)
        );

        sheet
            .change_claim(folds, at(12), |claim| {
                claim.edit("Folds flat in one move", at(12))
            })
            .unwrap();
        brief
            .use_claim(sheet.claim(folds).unwrap(), at(13))
            .unwrap();
        assert_eq!(
            texts(&brief),
            ["Lasts 40 hours on a charge", "Folds flat in one move"]
        );
        // One already used stays as the video has it.
        brief
            .use_claim(sheet.claim(folds).unwrap(), at(14))
            .unwrap();
        assert_eq!(brief.claims().len(), 2);
        assert_eq!(brief.changed_at(), at(13));
    }

    #[test]
    fn only_a_confirmed_claim_is_taken_into_a_video() {
        let product = product();
        let (sheet, _, _) = sheet(&product);
        let mut brief = brief(&product, &sheet);
        let rejected = sheet
            .claims()
            .iter()
            .find(|claim| claim.state() == ClaimState::Rejected)
            .unwrap();
        assert_eq!(
            brief.use_claim(rejected, at(11)),
            Err(BriefClaimError::NotConfirmed)
        );
        assert_eq!(brief.claims().len(), 2);
    }

    #[test]
    fn a_claim_edited_in_the_product_leaves_the_copy_and_asks() {
        let product = product();
        let (mut sheet, folds, _) = sheet(&product);
        let mut brief = brief(&product, &sheet);
        brief.confirm(&sheet, at(11)).unwrap();

        sheet
            .change_claim(folds, at(20), |claim| {
                claim.edit("Folds flat in one move", at(20))
            })
            .unwrap();
        assert_eq!(brief.claims()[0].text, "Folds flat");
        assert_eq!(
            brief.claims()[0].drift(&sheet),
            Some(ClaimDrift::Edited {
                now: "Folds flat in one move".into()
            })
        );
        assert_eq!(brief.claims()[1].drift(&sheet), None);
        assert_eq!(brief.claims_to_decide(&sheet), 1);

        // Keeping the copy answers this change, and marks nothing.
        brief.keep_claim(folds, &sheet).unwrap();
        assert_eq!(brief.claims_to_decide(&sheet), 0);
        assert_eq!(brief.claims()[0].text, "Folds flat");
        assert_eq!(brief.changed_at(), at(10));

        // Another edit there is another change.
        sheet
            .change_claim(folds, at(30), |claim| {
                claim.edit("Folds flat in two moves", at(30))
            })
            .unwrap();
        assert_eq!(brief.claims_to_decide(&sheet), 1);
    }

    #[test]
    fn a_claim_rejected_in_the_product_asks_and_dropping_takes_it_out() {
        let product = product();
        let (mut sheet, folds, lasts) = sheet(&product);
        let mut brief = brief(&product, &sheet);
        brief.confirm(&sheet, at(11)).unwrap();

        sheet.change_claim(lasts, at(20), Claim::reject).unwrap();
        assert_eq!(
            brief.claims()[1].drift(&sheet),
            Some(ClaimDrift::Unconfirmed)
        );
        assert_eq!(brief.claims_to_decide(&sheet), 1);

        brief.drop_claim(lasts, at(21)).unwrap();
        assert_eq!(brief.claims_to_decide(&sheet), 0);
        assert!(brief.uses(folds) && !brief.uses(lasts));
        assert_eq!(brief.changed_at(), at(21));
        assert_eq!(
            brief.keep_claim(lasts, &sheet),
            Err(BriefClaimError::NotUsed)
        );
    }

    #[test]
    fn a_kept_copy_is_asked_about_again_when_the_claim_comes_back_and_goes() {
        let product = product();
        let (mut sheet, folds, _) = sheet(&product);
        let mut brief = brief(&product, &sheet);

        sheet.change_claim(folds, at(20), Claim::reject).unwrap();
        brief.keep_claim(folds, &sheet).unwrap();
        assert_eq!(brief.claims_to_decide(&sheet), 0);
        assert!(!brief.settle(&sheet), "the product still says otherwise");

        // The user's own claim comes back confirmed: both agree again.
        sheet
            .change_claim(folds, at(21), |claim| claim.bring_back(at(21)))
            .unwrap();
        assert_eq!(brief.claims_to_decide(&sheet), 0);
        assert!(brief.settle(&sheet));
        assert_eq!(brief.claims()[0].kept, None);
        assert!(!brief.settle(&sheet));

        sheet.change_claim(folds, at(22), Claim::reject).unwrap();
        assert_eq!(brief.claims_to_decide(&sheet), 1);
    }

    #[test]
    fn a_changed_claim_stands_in_the_way_of_confirming() {
        let product = product();
        let (mut sheet, folds, _) = sheet(&product);
        let mut brief = brief(&product, &sheet);
        sheet.change_claim(folds, at(20), Claim::reject).unwrap();
        assert_eq!(
            brief.confirm(&sheet, at(21)),
            Err(BriefNotReady::ClaimsToDecide)
        );
        brief.keep_claim(folds, &sheet).unwrap();
        brief.confirm(&sheet, at(22)).unwrap();
    }

    #[test]
    fn a_change_after_a_script_was_written_marks_it_until_the_user_keeps_it() {
        let product = product();
        let (sheet, folds, _) = sheet(&product);
        let mut brief = brief(&product, &sheet);
        brief.confirm(&sheet, at(11)).unwrap();
        let written = at(100);
        assert!(!brief.outdates_script(written));

        // The same text again changes nothing.
        assert!(!brief.set_text(brief.text().clone(), at(200)));
        assert!(!brief.outdates_script(written));

        assert!(brief.set_text(
            BriefText::new("Two for one", "Order yours today", "").unwrap(),
            at(200)
        ));
        assert!(brief.outdates_script(written));
        // A script written after the change read it.
        assert!(!brief.outdates_script(at(300)));

        brief.keep_script(at(250));
        assert_eq!(brief.script_kept_at(), Some(at(250)));
        assert!(!brief.outdates_script(written));

        // A later change marks it again.
        brief.drop_claim(folds, at(400)).unwrap();
        assert!(brief.outdates_script(written));
        brief.keep_script(at(410));
        brief.touch(at(500));
        assert!(brief.outdates_script(written));
    }

    #[test]
    fn a_brief_round_trips_through_its_record() {
        let product = product();
        let (mut sheet, folds, _) = sheet(&product);
        let mut brief = brief(&product, &sheet);
        brief.confirm(&sheet, at(11)).unwrap();
        sheet.change_claim(folds, at(20), Claim::reject).unwrap();
        brief.keep_claim(folds, &sheet).unwrap();
        brief.keep_script(at(30));

        let record = AdBriefRecord {
            project: brief.project,
            product: brief.product,
            angle: brief.angle,
            text: brief.text().clone(),
            claims: brief.claims().to_vec(),
            confirmed_at: brief.confirmed_at(),
            changed_at: brief.changed_at(),
            script_kept_at: brief.script_kept_at(),
        };
        assert_eq!(AdBrief::restore(record), brief);
    }
}
