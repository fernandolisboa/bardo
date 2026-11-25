//! The product sheet and its claims (docs/spec/marketing-videos.md,
//! CONTEXT.md): what Bardo knows about a product after research, for the
//! user to review. A script may state about a product only claims the user
//! confirmed, and only a statement the user typed starts confirmed:
//! everything a model wrote waits, whatever source it names.

use std::time::SystemTime;

use crate::{PageRead, ProductId};

uuid_id!(
    /// Identifies one claim of a product sheet.
    ClaimId
);

/// What kind of statement a claim is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ClaimKind {
    /// What the customer gets out of it.
    Benefit,
    /// What the product has or does.
    #[default]
    Feature,
    /// A number, an award, a rating: something that can be checked.
    Proof,
    /// A price, a discount, a deadline.
    Offer,
    /// How it stands beside something else.
    Comparison,
}

impl ClaimKind {
    pub const ALL: [ClaimKind; 5] = [
        ClaimKind::Benefit,
        ClaimKind::Feature,
        ClaimKind::Proof,
        ClaimKind::Offer,
        ClaimKind::Comparison,
    ];

    /// Stable name stored in the database and asked of the model.
    pub fn code(self) -> &'static str {
        match self {
            ClaimKind::Benefit => "benefit",
            ClaimKind::Feature => "feature",
            ClaimKind::Proof => "proof",
            ClaimKind::Offer => "offer",
            ClaimKind::Comparison => "comparison",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.code() == code)
    }
}

/// Where a claim stands with the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClaimState {
    /// The user stands behind it: an ad may state it.
    Confirmed,
    /// A model wrote it; it waits for the user.
    Found,
    /// The user turned it down. Kept, so a later research does not bring
    /// it back as new.
    Rejected,
}

impl ClaimState {
    pub const ALL: [ClaimState; 3] = [
        ClaimState::Confirmed,
        ClaimState::Found,
        ClaimState::Rejected,
    ];

    pub fn code(self) -> &'static str {
        match self {
            ClaimState::Confirmed => "confirmed",
            ClaimState::Found => "found",
            ClaimState::Rejected => "rejected",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|state| state.code() == code)
    }
}

/// Where a claim came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimSource {
    /// The user typed it: a fact of the brief, or a claim added on the
    /// sheet.
    Typed,
    /// A model drew it from the brief's text. A paraphrase can change what
    /// was said, so it waits like any other.
    Brief,
    /// A model read it on the product's own page.
    ProductPage { url: String },
    /// A model read it on another page.
    Web { url: String, title: String },
}

impl ClaimSource {
    pub fn code(&self) -> &'static str {
        match self {
            ClaimSource::Typed => "typed",
            ClaimSource::Brief => "brief",
            ClaimSource::ProductPage { .. } => "page",
            ClaimSource::Web { .. } => "web",
        }
    }

    /// The source stored as `code` with its `url` and `title`.
    pub fn from_parts(code: &str, url: &str, title: &str) -> Option<Self> {
        match code {
            "typed" => Some(ClaimSource::Typed),
            "brief" => Some(ClaimSource::Brief),
            "page" => Some(ClaimSource::ProductPage {
                url: url.to_owned(),
            }),
            "web" => Some(ClaimSource::Web {
                url: url.to_owned(),
                title: title.to_owned(),
            }),
            _ => None,
        }
    }

    /// The page to open to check the claim, when it has one.
    pub fn url(&self) -> Option<&str> {
        match self {
            ClaimSource::ProductPage { url } | ClaimSource::Web { url, .. } => Some(url),
            ClaimSource::Typed | ClaimSource::Brief => None,
        }
    }

    pub fn title(&self) -> &str {
        match self {
            ClaimSource::Web { title, .. } => title,
            _ => "",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ClaimError {
    #[error("a claim needs its text")]
    TextRequired,
    #[error("a claim is one statement of at most 300 characters")]
    TextTooLong,
    /// Only a claim that waits can be confirmed.
    #[error("only a found claim can be confirmed")]
    NotFound,
    #[error("the claim is already rejected")]
    AlreadyRejected,
    #[error("only a rejected claim can be brought back")]
    NotRejected,
}

/// One statement about a product an ad could make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub id: ClaimId,
    text: String,
    pub kind: ClaimKind,
    source: ClaimSource,
    state: ClaimState,
    /// Whether the model that wrote it marked it sensitive.
    flagged: bool,
    /// The user's own word on whether it is sensitive, when they gave one.
    sensitive_choice: Option<bool>,
    /// When the user confirmed it; kept while it is confirmed.
    confirmed_at: Option<SystemTime>,
    /// When the user last changed its text.
    edited_at: Option<SystemTime>,
    pub created_at: SystemTime,
}

/// A claim as it is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimRecord {
    pub id: ClaimId,
    pub text: String,
    pub kind: ClaimKind,
    pub source: ClaimSource,
    pub state: ClaimState,
    pub flagged: bool,
    pub sensitive_choice: Option<bool>,
    pub confirmed_at: Option<SystemTime>,
    pub edited_at: Option<SystemTime>,
    pub created_at: SystemTime,
}

/// The statement as one line: line breaks and runs of spaces become one
/// space.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// How two statements are compared: without case, spacing or the
/// punctuation that ends them.
fn folded(text: &str) -> String {
    one_line(text)
        .trim_end_matches(['.', '!', ';', ':', ','])
        .to_lowercase()
}

impl Claim {
    pub const MAX_TEXT_CHARS: usize = 300;

    fn checked(text: &str) -> Result<String, ClaimError> {
        let text = one_line(text);
        if text.is_empty() {
            return Err(ClaimError::TextRequired);
        }
        if text.chars().count() > Self::MAX_TEXT_CHARS {
            return Err(ClaimError::TextTooLong);
        }
        Ok(text)
    }

    /// A claim the user typed: theirs, so it starts confirmed.
    pub fn typed(text: &str, kind: ClaimKind, now: SystemTime) -> Result<Self, ClaimError> {
        Ok(Self {
            id: ClaimId::new(),
            text: Self::checked(text)?,
            kind,
            source: ClaimSource::Typed,
            state: ClaimState::Confirmed,
            flagged: false,
            sensitive_choice: None,
            confirmed_at: Some(now),
            edited_at: None,
            created_at: now,
        })
    }

    /// A claim a model wrote: it waits for the user, whatever `source` it
    /// names. A model never writes a typed claim, so that source is read
    /// as the brief's text.
    pub fn found(
        text: &str,
        kind: ClaimKind,
        source: ClaimSource,
        flagged: bool,
        now: SystemTime,
    ) -> Result<Self, ClaimError> {
        let source = match source {
            ClaimSource::Typed => ClaimSource::Brief,
            source => source,
        };
        Ok(Self {
            id: ClaimId::new(),
            text: Self::checked(text)?,
            kind,
            source,
            state: ClaimState::Found,
            flagged,
            sensitive_choice: None,
            confirmed_at: None,
            edited_at: None,
            created_at: now,
        })
    }

    /// A stored claim, or `None` when what is stored cannot be one.
    pub fn restore(record: ClaimRecord) -> Option<Self> {
        let text = Self::checked(&record.text).ok()?;
        Some(Self {
            id: record.id,
            text,
            kind: record.kind,
            source: record.source,
            state: record.state,
            flagged: record.flagged,
            sensitive_choice: record.sensitive_choice,
            confirmed_at: record.confirmed_at,
            edited_at: record.edited_at,
            created_at: record.created_at,
        })
    }

    pub fn record(&self) -> ClaimRecord {
        ClaimRecord {
            id: self.id,
            text: self.text.clone(),
            kind: self.kind,
            source: self.source.clone(),
            state: self.state,
            flagged: self.flagged,
            sensitive_choice: self.sensitive_choice,
            confirmed_at: self.confirmed_at,
            edited_at: self.edited_at,
            created_at: self.created_at,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn source(&self) -> &ClaimSource {
        &self.source
    }

    pub fn state(&self) -> ClaimState {
        self.state
    }

    pub fn confirmed_at(&self) -> Option<SystemTime> {
        self.confirmed_at
    }

    pub fn edited_at(&self) -> Option<SystemTime> {
        self.edited_at
    }

    /// Whether the claim says the same as `text`.
    pub fn says(&self, text: &str) -> bool {
        folded(&self.text) == folded(text)
    }

    /// Whether an ad that uses the claim lists it again for the user: a
    /// claim with a number in it, or one the model marked (a superlative,
    /// a health, money or results promise, a guarantee, a deadline, a
    /// comparison that names a competitor), unless the user said
    /// otherwise, either way.
    pub fn sensitive(&self) -> bool {
        self.sensitive_choice
            .unwrap_or_else(|| self.flagged || self.text.chars().any(char::is_numeric))
    }

    /// Whether the user set [`Claim::sensitive`] themselves.
    pub fn sensitive_chosen(&self) -> bool {
        self.sensitive_choice.is_some()
    }

    /// The user's own word on whether the claim is sensitive.
    pub fn set_sensitive(&mut self, sensitive: bool) {
        self.sensitive_choice = Some(sensitive);
    }

    /// The user stands behind a claim that waited.
    pub fn confirm(&mut self, now: SystemTime) -> Result<(), ClaimError> {
        if self.state != ClaimState::Found {
            return Err(ClaimError::NotFound);
        }
        self.state = ClaimState::Confirmed;
        self.confirmed_at = Some(now);
        Ok(())
    }

    /// The user turns down a claim, found or confirmed.
    pub fn reject(&mut self) -> Result<(), ClaimError> {
        if self.state == ClaimState::Rejected {
            return Err(ClaimError::AlreadyRejected);
        }
        self.state = ClaimState::Rejected;
        self.confirmed_at = None;
        Ok(())
    }

    /// Brings a rejected claim back: one the user typed is theirs again,
    /// one a model wrote waits again.
    pub fn bring_back(&mut self, now: SystemTime) -> Result<(), ClaimError> {
        if self.state != ClaimState::Rejected {
            return Err(ClaimError::NotRejected);
        }
        if self.source == ClaimSource::Typed {
            self.state = ClaimState::Confirmed;
            self.confirmed_at = Some(now);
        } else {
            self.state = ClaimState::Found;
        }
        Ok(())
    }

    /// The user's own wording: the claim stays in its state and keeps its
    /// source, with the time of the edit. Text that says the same is no
    /// edit. A number written into a claim that had none makes it
    /// sensitive again, even where the user had said it was not: their
    /// word was about a claim without a number.
    pub fn edit(&mut self, text: &str, now: SystemTime) -> Result<(), ClaimError> {
        let text = Self::checked(text)?;
        if text != self.text {
            let numbered = |text: &str| text.chars().any(char::is_numeric);
            if self.sensitive_choice == Some(false) && numbered(&text) && !numbered(&self.text) {
                self.sensitive_choice = None;
            }
            self.text = text;
            self.edited_at = Some(now);
        }
        Ok(())
    }

    /// The user typed this very statement in the brief: the claim is
    /// theirs from now on, and confirmed, whatever a model had made of it
    /// and whatever the user had said of the model's claim.
    fn make_typed(&mut self, now: SystemTime) {
        self.source = ClaimSource::Typed;
        if self.state != ClaimState::Confirmed {
            self.state = ClaimState::Confirmed;
            self.confirmed_at = Some(now);
        }
    }
}

/// A category of product the networks have their own rules for. Bardo
/// shows a notice; it does not refuse the product.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RestrictedCategory {
    Health,
    Finance,
    Alcohol,
    Gambling,
    WeightLoss,
    Adult,
    Political,
}

impl RestrictedCategory {
    pub const ALL: [RestrictedCategory; 7] = [
        RestrictedCategory::Health,
        RestrictedCategory::Finance,
        RestrictedCategory::Alcohol,
        RestrictedCategory::Gambling,
        RestrictedCategory::WeightLoss,
        RestrictedCategory::Adult,
        RestrictedCategory::Political,
    ];

    pub fn code(self) -> &'static str {
        match self {
            RestrictedCategory::Health => "health",
            RestrictedCategory::Finance => "finance",
            RestrictedCategory::Alcohol => "alcohol",
            RestrictedCategory::Gambling => "gambling",
            RestrictedCategory::WeightLoss => "weight_loss",
            RestrictedCategory::Adult => "adult",
            RestrictedCategory::Political => "political",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|category| category.code() == code)
    }
}

/// Some of the people the product is for.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AudienceSegment {
    pub name: String,
    /// What they want.
    pub wants: String,
    /// What holds them back.
    pub holds_back: String,
}

/// Something a buyer says against the product, and what answers it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Objection {
    pub objection: String,
    pub answer: String,
}

/// What a buyer could choose instead, and how the product differs. Never
/// another brand's ad copy or slogan.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Alternative {
    pub name: String,
    pub differs: String,
}

/// A page a research read for the sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetSource {
    pub url: String,
    pub title: String,
    pub read_at: SystemTime,
}

/// The parts of a sheet that describe the product, as the user or a model
/// writes them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SheetText {
    pub summary: String,
    pub category: String,
    pub restricted: Option<RestrictedCategory>,
    pub segments: Vec<AudienceSegment>,
    pub objections: Vec<Objection>,
    pub alternatives: Vec<Alternative>,
    /// How the product looks, from its images.
    pub looks: String,
}

/// What is wrong with a sheet's text as the user typed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum SheetFieldError {
    #[error("the summary is too long")]
    SummaryTooLong,
    #[error("the category is too long")]
    CategoryTooLong,
    #[error("the description of the look is too long")]
    LooksTooLong,
    #[error("too many audience segments")]
    TooManySegments,
    #[error("an audience segment needs a name, and each of its parts has a limit")]
    Segment,
    #[error("too many objections")]
    TooManyObjections,
    #[error("an objection needs its text, and each of its parts has a limit")]
    Objection,
    #[error("too many alternatives")]
    TooManyAlternatives,
    #[error("an alternative needs a name, and each of its parts has a limit")]
    Alternative,
}

impl SheetFieldError {
    /// In the order the sheet shows its parts.
    pub const ALL: [SheetFieldError; 9] = [
        SheetFieldError::SummaryTooLong,
        SheetFieldError::CategoryTooLong,
        SheetFieldError::TooManySegments,
        SheetFieldError::Segment,
        SheetFieldError::TooManyObjections,
        SheetFieldError::Objection,
        SheetFieldError::TooManyAlternatives,
        SheetFieldError::Alternative,
        SheetFieldError::LooksTooLong,
    ];
}

fn clip(text: &str, max: usize) -> String {
    text.trim()
        .chars()
        .take(max)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

fn fits(text: &str, max: usize) -> bool {
    text.chars().count() <= max
}

impl SheetText {
    pub const MAX_SUMMARY_CHARS: usize = 1_500;
    pub const MAX_CATEGORY_CHARS: usize = 80;
    pub const MAX_LOOKS_CHARS: usize = 800;
    pub const MAX_NAME_CHARS: usize = 120;
    pub const MAX_PART_CHARS: usize = 400;
    pub const MAX_SEGMENTS: usize = 8;
    pub const MAX_OBJECTIONS: usize = 12;
    pub const MAX_ALTERNATIVES: usize = 12;

    /// The text trimmed, with rows that are wholly empty dropped.
    fn tidy(self) -> Self {
        let line = |text: &str| one_line(text);
        Self {
            summary: self.summary.trim().to_owned(),
            category: line(&self.category),
            restricted: self.restricted,
            segments: self
                .segments
                .into_iter()
                .map(|segment| AudienceSegment {
                    name: line(&segment.name),
                    wants: segment.wants.trim().to_owned(),
                    holds_back: segment.holds_back.trim().to_owned(),
                })
                .filter(|segment| *segment != AudienceSegment::default())
                .collect(),
            objections: self
                .objections
                .into_iter()
                .map(|objection| Objection {
                    objection: objection.objection.trim().to_owned(),
                    answer: objection.answer.trim().to_owned(),
                })
                .filter(|objection| *objection != Objection::default())
                .collect(),
            alternatives: self
                .alternatives
                .into_iter()
                .map(|alternative| Alternative {
                    name: line(&alternative.name),
                    differs: alternative.differs.trim().to_owned(),
                })
                .filter(|alternative| *alternative != Alternative::default())
                .collect(),
            looks: self.looks.trim().to_owned(),
        }
    }

    /// The text as the user typed it, tidied, or every problem in it, in
    /// the order the sheet shows its parts.
    pub fn validate(self) -> Result<Self, Vec<SheetFieldError>> {
        let text = self.tidy();
        let mut errors = Vec::new();
        if !fits(&text.summary, Self::MAX_SUMMARY_CHARS) {
            errors.push(SheetFieldError::SummaryTooLong);
        }
        if !fits(&text.category, Self::MAX_CATEGORY_CHARS) {
            errors.push(SheetFieldError::CategoryTooLong);
        }
        if text.segments.len() > Self::MAX_SEGMENTS {
            errors.push(SheetFieldError::TooManySegments);
        }
        if text.segments.iter().any(|segment| {
            segment.name.is_empty()
                || !fits(&segment.name, Self::MAX_NAME_CHARS)
                || !fits(&segment.wants, Self::MAX_PART_CHARS)
                || !fits(&segment.holds_back, Self::MAX_PART_CHARS)
        }) {
            errors.push(SheetFieldError::Segment);
        }
        if text.objections.len() > Self::MAX_OBJECTIONS {
            errors.push(SheetFieldError::TooManyObjections);
        }
        if text.objections.iter().any(|objection| {
            objection.objection.is_empty()
                || !fits(&objection.objection, Self::MAX_PART_CHARS)
                || !fits(&objection.answer, Self::MAX_PART_CHARS)
        }) {
            errors.push(SheetFieldError::Objection);
        }
        if text.alternatives.len() > Self::MAX_ALTERNATIVES {
            errors.push(SheetFieldError::TooManyAlternatives);
        }
        if text.alternatives.iter().any(|alternative| {
            alternative.name.is_empty()
                || !fits(&alternative.name, Self::MAX_NAME_CHARS)
                || !fits(&alternative.differs, Self::MAX_PART_CHARS)
        }) {
            errors.push(SheetFieldError::Alternative);
        }
        if !fits(&text.looks, Self::MAX_LOOKS_CHARS) {
            errors.push(SheetFieldError::LooksTooLong);
        }
        if errors.is_empty() {
            Ok(text)
        } else {
            Err(errors)
        }
    }

    /// The text of a model's answer made to fit: what is too long is cut
    /// at its limit, rows without their first part are dropped, and rows
    /// past the limit are left out. A paid answer is kept, not refused.
    pub fn fitted(self) -> Self {
        let text = self.tidy();
        Self {
            summary: clip(&text.summary, Self::MAX_SUMMARY_CHARS),
            category: clip(&text.category, Self::MAX_CATEGORY_CHARS),
            restricted: text.restricted,
            segments: text
                .segments
                .into_iter()
                .filter(|segment| !segment.name.is_empty())
                .take(Self::MAX_SEGMENTS)
                .map(|segment| AudienceSegment {
                    name: clip(&segment.name, Self::MAX_NAME_CHARS),
                    wants: clip(&segment.wants, Self::MAX_PART_CHARS),
                    holds_back: clip(&segment.holds_back, Self::MAX_PART_CHARS),
                })
                .collect(),
            objections: text
                .objections
                .into_iter()
                .filter(|objection| !objection.objection.is_empty())
                .take(Self::MAX_OBJECTIONS)
                .map(|objection| Objection {
                    objection: clip(&objection.objection, Self::MAX_PART_CHARS),
                    answer: clip(&objection.answer, Self::MAX_PART_CHARS),
                })
                .collect(),
            alternatives: text
                .alternatives
                .into_iter()
                .filter(|alternative| !alternative.name.is_empty())
                .take(Self::MAX_ALTERNATIVES)
                .map(|alternative| Alternative {
                    name: clip(&alternative.name, Self::MAX_NAME_CHARS),
                    differs: clip(&alternative.differs, Self::MAX_PART_CHARS),
                })
                .collect(),
            looks: clip(&text.looks, Self::MAX_LOOKS_CHARS),
        }
    }
}

/// Where a model says it took a claim from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WrittenSource {
    /// The brief's own text.
    Brief,
    /// A page, by its address.
    Page(String),
}

/// A claim as a model wrote it in its answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenClaim {
    pub text: String,
    pub kind: ClaimKind,
    pub source: WrittenSource,
    /// Whether the model marked it sensitive.
    pub sensitive: bool,
}

/// A sheet as a model wrote it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WrittenSheet {
    pub text: SheetText,
    pub claims: Vec<WrittenClaim>,
}

/// What taking a model's sheet in did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SheetTaken {
    /// Claims added, each waiting for the user.
    pub claims_added: usize,
    /// Claims the sheet already had, in whatever state.
    pub claims_known: usize,
    /// Claims left out: no page of the research backs them, or their text
    /// cannot be a claim.
    pub claims_dropped: usize,
    /// Claims left out because the sheet is full.
    pub claims_over: usize,
}

/// An address as pages are matched by: without scheme, `www.`, fragment
/// or a slash at its end, and without case in its host.
fn page_key(url: &str) -> String {
    let url = url.trim();
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let rest = rest.split('#').next().unwrap_or_default();
    let (host, path) = match rest.find(['/', '?']) {
        Some(at) => rest.split_at(at),
        None => (rest, ""),
    };
    let host = host.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    format!("{host}{}", path.trim_end_matches('/'))
}

/// Whether `page` is the product's own page: the address of the brief's
/// `link`, or one under it. A link that is only a site makes every page of
/// the site the product's; a link to one page of a shop does not make the
/// shop's other pages the product's, and a link with a query names one
/// page.
fn is_product_page(link: &str, page: &str) -> bool {
    let link = page_key(link);
    let page = page_key(page);
    if link.is_empty() {
        return false;
    }
    if page == link {
        return true;
    }
    let path = page.split('?').next().unwrap_or_default();
    !link.contains('?') && (path == link || path.starts_with(&format!("{link}/")))
}

/// What Bardo knows about a product, for the user to review and edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductSheet {
    pub product: ProductId,
    text: SheetText,
    claims: Vec<Claim>,
    sources: Vec<SheetSource>,
    /// Whether the user changed the restricted category themselves: what
    /// they chose stays, "none" included, whatever a later run finds.
    restricted_chosen: bool,
    /// When a research last read the web for it.
    pub researched_at: Option<SystemTime>,
    pub updated_at: SystemTime,
}

impl ProductSheet {
    /// The most claims a sheet holds, rejected ones aside: a rejected
    /// claim is kept so that no later run brings it back as new, and it
    /// takes no place.
    pub const MAX_CLAIMS: usize = 80;
    /// The most pages a sheet lists.
    pub const MAX_SOURCES: usize = 40;

    /// A sheet with nothing on it yet, to fill by hand.
    pub fn empty(product: ProductId, now: SystemTime) -> Self {
        Self {
            product,
            text: SheetText::default(),
            claims: Vec::new(),
            sources: Vec::new(),
            restricted_chosen: false,
            researched_at: None,
            updated_at: now,
        }
    }

    /// A stored sheet.
    pub fn restore(
        product: ProductId,
        text: SheetText,
        claims: Vec<Claim>,
        sources: Vec<SheetSource>,
        restricted_chosen: bool,
        researched_at: Option<SystemTime>,
        updated_at: SystemTime,
    ) -> Self {
        Self {
            product,
            text,
            claims,
            sources,
            restricted_chosen,
            researched_at,
            updated_at,
        }
    }

    /// Whether the user chose the restricted category themselves.
    pub fn restricted_chosen(&self) -> bool {
        self.restricted_chosen
    }

    pub fn text(&self) -> &SheetText {
        &self.text
    }

    pub fn claims(&self) -> &[Claim] {
        &self.claims
    }

    pub fn sources(&self) -> &[SheetSource] {
        &self.sources
    }

    pub fn claim(&self, id: ClaimId) -> Option<&Claim> {
        self.claims.iter().find(|claim| claim.id == id)
    }

    /// The claims an ad may state.
    pub fn confirmed(&self) -> impl Iterator<Item = &Claim> {
        self.claims
            .iter()
            .filter(|claim| claim.state() == ClaimState::Confirmed)
    }

    /// How many claims wait for the user.
    pub fn waiting(&self) -> usize {
        self.claims
            .iter()
            .filter(|claim| claim.state() == ClaimState::Found)
            .count()
    }

    fn knows(&self, text: &str) -> bool {
        self.claims.iter().any(|claim| claim.says(text))
    }

    /// Whether the sheet has no place for one more claim.
    fn is_full(&self) -> bool {
        let held = self
            .claims
            .iter()
            .filter(|claim| claim.state() != ClaimState::Rejected)
            .count();
        held >= Self::MAX_CLAIMS
    }

    /// The user's own text for the parts that describe the product.
    pub fn set_text(
        &mut self,
        text: SheetText,
        now: SystemTime,
    ) -> Result<(), Vec<SheetFieldError>> {
        let text = text.validate()?;
        if text.restricted != self.text.restricted {
            self.restricted_chosen = true;
        }
        self.text = text;
        self.updated_at = now;
        Ok(())
    }

    /// Adds the facts of the brief the sheet does not hold yet, each as a
    /// confirmed claim the user typed, before the claims models wrote. A
    /// fact a model's claim already says makes that claim the user's own:
    /// typed and confirmed, wherever it stood. Returns how many claims it
    /// added or made the user's. A fact taken out of the brief leaves its
    /// claim: the user rejects it on the sheet, and a typed claim they
    /// rejected stays rejected.
    pub fn take_facts(&mut self, facts: &[String], now: SystemTime) -> usize {
        let mut added = 0;
        for fact in facts {
            if let Some(known) = self.claims.iter_mut().find(|claim| claim.says(fact)) {
                if *known.source() != ClaimSource::Typed {
                    known.make_typed(now);
                    added += 1;
                }
                continue;
            }
            if self.is_full() {
                continue;
            }
            let Ok(claim) = Claim::typed(fact, ClaimKind::Feature, now) else {
                continue;
            };
            // After the typed claims already there, before the others.
            let at = self
                .claims
                .iter()
                .rposition(|claim| *claim.source() == ClaimSource::Typed)
                .map_or(0, |last| last + 1);
            self.claims.insert(at, claim);
            added += 1;
        }
        if added > 0 {
            self.updated_at = now;
        }
        added
    }

    /// A claim the user adds on the sheet: theirs, so confirmed.
    pub fn add_claim(
        &mut self,
        text: &str,
        kind: ClaimKind,
        now: SystemTime,
    ) -> Result<ClaimId, SheetClaimError> {
        if self.is_full() {
            return Err(SheetClaimError::TooMany);
        }
        let claim = Claim::typed(text, kind, now)?;
        if self.knows(claim.text()) {
            return Err(SheetClaimError::Known);
        }
        let id = claim.id;
        self.claims.push(claim);
        self.updated_at = now;
        Ok(id)
    }

    /// Changes one claim with `change`.
    pub fn change_claim(
        &mut self,
        id: ClaimId,
        now: SystemTime,
        change: impl FnOnce(&mut Claim) -> Result<(), ClaimError>,
    ) -> Result<(), SheetClaimError> {
        let at = self
            .claims
            .iter()
            .position(|claim| claim.id == id)
            .ok_or(SheetClaimError::Gone)?;
        let mut changed = self.claims[at].clone();
        change(&mut changed)?;
        // Two claims never say the same.
        if self
            .claims
            .iter()
            .any(|other| other.id != id && other.says(changed.text()))
        {
            return Err(SheetClaimError::Known);
        }
        // A rejected claim comes back only where there is a place for it.
        let returns = self.claims[at].state() == ClaimState::Rejected
            && changed.state() != ClaimState::Rejected;
        if returns && self.is_full() {
            return Err(SheetClaimError::TooMany);
        }
        self.claims[at] = changed;
        self.updated_at = now;
        Ok(())
    }

    /// Takes in a sheet a model wrote from the brief and, after a
    /// research, from the `pages` it read.
    ///
    /// It adds what is new and changes nothing the sheet holds: a part
    /// that has text keeps it, rows are added by their name, and a claim
    /// the sheet knows stays as it is, confirmed, rejected or waiting.
    /// Every claim taken in waits for the user, whatever source the model
    /// gave it. One that names a page the research did not read is left
    /// out, and so is one that names a page when there was no research.
    /// A claim that names the brief is the model's reading of it, which
    /// nothing here checks: the sheet says so beside it.
    pub fn take_written(
        &mut self,
        written: WrittenSheet,
        pages: &[PageRead],
        product_link: &str,
        now: SystemTime,
    ) -> SheetTaken {
        let written_text = written.text.fitted();
        let same = |a: &str, b: &str| folded(a) == folded(b);
        if self.text.summary.is_empty() {
            self.text.summary = written_text.summary;
        }
        if self.text.category.is_empty() {
            self.text.category = written_text.category;
        }
        if self.text.looks.is_empty() {
            self.text.looks = written_text.looks;
        }
        // A category networks restrict is said once and stays until the
        // user changes it, and what the user chose stays, "none" too.
        if self.text.restricted.is_none() && !self.restricted_chosen {
            self.text.restricted = written_text.restricted;
        }
        for segment in written_text.segments {
            if self.text.segments.len() < SheetText::MAX_SEGMENTS
                && !self
                    .text
                    .segments
                    .iter()
                    .any(|held| same(&held.name, &segment.name))
            {
                self.text.segments.push(segment);
            }
        }
        for objection in written_text.objections {
            if self.text.objections.len() < SheetText::MAX_OBJECTIONS
                && !self
                    .text
                    .objections
                    .iter()
                    .any(|held| same(&held.objection, &objection.objection))
            {
                self.text.objections.push(objection);
            }
        }
        for alternative in written_text.alternatives {
            if self.text.alternatives.len() < SheetText::MAX_ALTERNATIVES
                && !self
                    .text
                    .alternatives
                    .iter()
                    .any(|held| same(&held.name, &alternative.name))
            {
                self.text.alternatives.push(alternative);
            }
        }

        let mut taken = SheetTaken::default();
        for claim in written.claims {
            let source = match &claim.source {
                WrittenSource::Brief => Some(ClaimSource::Brief),
                WrittenSource::Page(url) => {
                    let key = page_key(url);
                    pages
                        .iter()
                        .find(|page| page_key(&page.url) == key)
                        .map(|page| {
                            if is_product_page(product_link, &page.url) {
                                ClaimSource::ProductPage {
                                    url: page.url.clone(),
                                }
                            } else {
                                ClaimSource::Web {
                                    url: page.url.clone(),
                                    title: page.title.clone(),
                                }
                            }
                        })
                }
            };
            let Some(source) = source else {
                taken.claims_dropped += 1;
                continue;
            };
            let Ok(claim) = Claim::found(&claim.text, claim.kind, source, claim.sensitive, now)
            else {
                taken.claims_dropped += 1;
                continue;
            };
            if self.knows(claim.text()) {
                taken.claims_known += 1;
                continue;
            }
            if self.is_full() {
                taken.claims_over += 1;
                continue;
            }
            self.claims.push(claim);
            taken.claims_added += 1;
        }

        for page in pages {
            let key = page_key(&page.url);
            if self.sources.len() < Self::MAX_SOURCES
                && !self.sources.iter().any(|held| page_key(&held.url) == key)
            {
                self.sources.push(SheetSource {
                    url: page.url.clone(),
                    title: page.title.clone(),
                    read_at: page.read_at.unwrap_or(now),
                });
            }
        }
        if !pages.is_empty() {
            self.researched_at = Some(now);
        }
        self.updated_at = now;
        taken
    }

    /// The sheet as another product's, its claims under their own ids.
    pub fn for_product(&self, product: ProductId, now: SystemTime) -> Self {
        let mut copy = self.clone();
        copy.product = product;
        for claim in &mut copy.claims {
            claim.id = ClaimId::new();
        }
        copy.updated_at = now;
        copy
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SheetClaimError {
    #[error(transparent)]
    Claim(#[from] ClaimError),
    #[error("the sheet already has a claim that says this")]
    Known,
    #[error("the sheet has as many claims as it holds, rejected ones aside")]
    TooMany,
    #[error("the claim is gone")]
    Gone,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn at(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + seconds)
    }

    fn found(text: &str, flagged: bool) -> Claim {
        Claim::found(text, ClaimKind::Benefit, ClaimSource::Brief, flagged, at(0)).unwrap()
    }

    fn page(url: &str, title: &str) -> PageRead {
        PageRead {
            url: url.to_owned(),
            title: title.to_owned(),
            read_at: Some(at(5)),
        }
    }

    fn written(text: &str, source: WrittenSource) -> WrittenClaim {
        WrittenClaim {
            text: text.to_owned(),
            kind: ClaimKind::Feature,
            source,
            sensitive: false,
        }
    }

    #[test]
    fn only_a_claim_the_user_typed_starts_confirmed() {
        let typed = Claim::typed("  Ships in\ntwo days ", ClaimKind::Offer, at(1)).unwrap();
        assert_eq!(typed.text(), "Ships in two days");
        assert_eq!(typed.state(), ClaimState::Confirmed);
        assert_eq!(typed.confirmed_at(), Some(at(1)));
        assert_eq!(*typed.source(), ClaimSource::Typed);

        for source in [
            ClaimSource::Brief,
            ClaimSource::ProductPage {
                url: "https://lamp.example".into(),
            },
            ClaimSource::Web {
                url: "https://review.example/lamp".into(),
                title: "A review".into(),
            },
        ] {
            let claim = Claim::found(
                "Lights a desk",
                ClaimKind::Benefit,
                source.clone(),
                false,
                at(1),
            )
            .unwrap();
            assert_eq!(claim.state(), ClaimState::Found);
            assert_eq!(claim.confirmed_at(), None);
            assert_eq!(*claim.source(), source);
        }
        // A model cannot write a typed claim.
        let claim = Claim::found(
            "Lights a desk",
            ClaimKind::Benefit,
            ClaimSource::Typed,
            false,
            at(1),
        )
        .unwrap();
        assert_eq!(*claim.source(), ClaimSource::Brief);
        assert_eq!(claim.state(), ClaimState::Found);
    }

    #[test]
    fn a_claim_is_one_statement_within_its_limit() {
        assert_eq!(
            Claim::typed(" \n ", ClaimKind::Feature, at(0)),
            Err(ClaimError::TextRequired)
        );
        let long = "a".repeat(Claim::MAX_TEXT_CHARS + 1);
        assert_eq!(
            Claim::typed(&long, ClaimKind::Feature, at(0)),
            Err(ClaimError::TextTooLong)
        );
        assert!(
            Claim::typed(
                &"á".repeat(Claim::MAX_TEXT_CHARS),
                ClaimKind::Feature,
                at(0)
            )
            .is_ok()
        );
    }

    #[test]
    fn a_found_claim_is_confirmed_or_rejected_and_a_confirmed_one_rejected() {
        let mut claim = found("Lights a desk", false);
        claim.confirm(at(10)).unwrap();
        assert_eq!(claim.state(), ClaimState::Confirmed);
        assert_eq!(claim.confirmed_at(), Some(at(10)));
        // It keeps where it came from.
        assert_eq!(*claim.source(), ClaimSource::Brief);
        assert_eq!(claim.confirm(at(11)), Err(ClaimError::NotFound));

        claim.reject().unwrap();
        assert_eq!(claim.state(), ClaimState::Rejected);
        assert_eq!(claim.confirmed_at(), None);
        assert_eq!(claim.reject(), Err(ClaimError::AlreadyRejected));
        assert_eq!(claim.confirm(at(12)), Err(ClaimError::NotFound));

        let mut waiting = found("Dims by touch", false);
        waiting.reject().unwrap();
        assert_eq!(waiting.state(), ClaimState::Rejected);
    }

    #[test]
    fn a_rejected_claim_comes_back_waiting_unless_the_user_typed_it() {
        let mut claim = found("Lights a desk", false);
        assert_eq!(claim.bring_back(at(1)), Err(ClaimError::NotRejected));
        claim.reject().unwrap();
        claim.bring_back(at(20)).unwrap();
        assert_eq!(claim.state(), ClaimState::Found);
        assert_eq!(claim.confirmed_at(), None);

        let mut typed = Claim::typed("Made of steel", ClaimKind::Feature, at(1)).unwrap();
        typed.reject().unwrap();
        typed.bring_back(at(20)).unwrap();
        assert_eq!(typed.state(), ClaimState::Confirmed);
        assert_eq!(typed.confirmed_at(), Some(at(20)));
    }

    #[test]
    fn editing_keeps_the_state_and_records_the_time() {
        let mut claim = found("Lights a desk", false);
        claim.edit("Lights a whole desk", at(30)).unwrap();
        assert_eq!(claim.text(), "Lights a whole desk");
        assert_eq!(claim.state(), ClaimState::Found);
        assert_eq!(claim.edited_at(), Some(at(30)));
        assert_eq!(*claim.source(), ClaimSource::Brief);

        claim.confirm(at(31)).unwrap();
        claim.edit("Lights the whole desk", at(40)).unwrap();
        assert_eq!(claim.state(), ClaimState::Confirmed);
        assert_eq!(claim.confirmed_at(), Some(at(31)));
        assert_eq!(claim.edited_at(), Some(at(40)));

        // The same text is no edit, and a bad one changes nothing.
        claim.edit(" Lights the  whole desk ", at(50)).unwrap();
        assert_eq!(claim.edited_at(), Some(at(40)));
        assert_eq!(claim.edit("", at(60)), Err(ClaimError::TextRequired));
        assert_eq!(claim.text(), "Lights the whole desk");
    }

    #[test]
    fn a_digit_or_the_models_mark_makes_a_claim_sensitive_until_the_user_says() {
        // Neither a digit nor a mark.
        let mut plain = found("Lights a desk", false);
        assert!(!plain.sensitive());
        // The model's mark.
        let mut marked = found("The best lamp there is", true);
        assert!(marked.sensitive());
        // A digit, whatever the model said; other scripts' digits too.
        let mut number = found("Lasts 40 hours", false);
        assert!(number.sensitive());
        assert!(found("Dura ٤٠ horas", false).sensitive());
        assert!(!number.sensitive_chosen());

        // The user switches it either way.
        number.set_sensitive(false);
        assert!(!number.sensitive());
        assert!(number.sensitive_chosen());
        marked.set_sensitive(false);
        assert!(!marked.sensitive());
        plain.set_sensitive(true);
        assert!(plain.sensitive());

        // An edit that adds a number turns it on where the user gave no
        // word, and leaves the user's word alone.
        let mut edited = found("Lasts for days", false);
        edited.edit("Lasts 3 days", at(1)).unwrap();
        assert!(edited.sensitive());
        number.edit("Lasts 48 hours", at(1)).unwrap();
        assert!(!number.sensitive());

        // The user's "not sensitive" was about a claim without a number:
        // a number written into it makes the claim sensitive again.
        let mut cleared = found("The best lamp there is", true);
        cleared.set_sensitive(false);
        cleared.edit("The best lamp of 2026", at(2)).unwrap();
        assert!(cleared.sensitive());
        assert!(!cleared.sensitive_chosen());
        // And their word given after that stands.
        cleared.set_sensitive(false);
        cleared.edit("The best lamp of 2027", at(3)).unwrap();
        assert!(!cleared.sensitive());
    }

    #[test]
    fn a_stored_claim_reads_back_as_it_was() {
        let mut claim = Claim::found(
            "Rated 4.8 by buyers",
            ClaimKind::Proof,
            ClaimSource::Web {
                url: "https://review.example/lamp".into(),
                title: "A review".into(),
            },
            true,
            at(1),
        )
        .unwrap();
        claim.confirm(at(2)).unwrap();
        claim.edit("Rated 4.8 by its buyers", at(3)).unwrap();
        claim.set_sensitive(false);
        assert_eq!(Claim::restore(claim.record()), Some(claim.clone()));

        let mut broken = claim.record();
        broken.text = String::new();
        assert_eq!(Claim::restore(broken), None);
    }

    #[test]
    fn codes_read_back() {
        for kind in ClaimKind::ALL {
            assert_eq!(ClaimKind::from_code(kind.code()), Some(kind));
        }
        for state in ClaimState::ALL {
            assert_eq!(ClaimState::from_code(state.code()), Some(state));
        }
        for category in RestrictedCategory::ALL {
            assert_eq!(
                RestrictedCategory::from_code(category.code()),
                Some(category)
            );
        }
        assert_eq!(ClaimKind::from_code("testimonial"), None);
        for source in [
            ClaimSource::Typed,
            ClaimSource::Brief,
            ClaimSource::ProductPage { url: "u".into() },
            ClaimSource::Web {
                url: "u".into(),
                title: "t".into(),
            },
        ] {
            assert_eq!(
                ClaimSource::from_parts(source.code(), source.url().unwrap_or(""), source.title()),
                Some(source)
            );
        }
        assert_eq!(ClaimSource::from_parts("rumor", "", ""), None);
    }

    #[test]
    fn the_briefs_facts_become_confirmed_claims_once() {
        let mut sheet = ProductSheet::empty(ProductId::new(), at(0));
        let facts = vec!["Made of steel".to_owned(), "Ships in 2 days".to_owned()];
        assert_eq!(sheet.take_facts(&facts, at(1)), 2);
        assert_eq!(sheet.take_facts(&facts, at(2)), 0);
        assert_eq!(sheet.updated_at, at(1));
        let claims = sheet.claims();
        assert_eq!(claims.len(), 2);
        assert!(claims.iter().all(|claim| {
            claim.state() == ClaimState::Confirmed && *claim.source() == ClaimSource::Typed
        }));
        assert_eq!(claims[0].text(), "Made of steel");

        // A new fact goes after the typed ones, before what a model wrote;
        // the same fact in other letters or with a full stop is known.
        sheet.take_written(
            WrittenSheet {
                claims: vec![written("Lights a desk", WrittenSource::Brief)],
                ..WrittenSheet::default()
            },
            &[],
            "",
            at(3),
        );
        let more = vec!["made of STEEL.".to_owned(), "Dims by touch".to_owned()];
        assert_eq!(sheet.take_facts(&more, at(4)), 1);
        let texts: Vec<&str> = sheet.claims().iter().map(Claim::text).collect();
        assert_eq!(
            texts,
            [
                "Made of steel",
                "Ships in 2 days",
                "Dims by touch",
                "Lights a desk"
            ]
        );
    }

    #[test]
    fn a_rejected_fact_is_not_added_again() {
        let mut sheet = ProductSheet::empty(ProductId::new(), at(0));
        let facts = vec!["Made of steel".to_owned()];
        sheet.take_facts(&facts, at(1));
        let id = sheet.claims()[0].id;
        sheet.change_claim(id, at(2), Claim::reject).unwrap();
        assert_eq!(sheet.take_facts(&facts, at(3)), 0);
        assert_eq!(sheet.claims()[0].state(), ClaimState::Rejected);
    }

    #[test]
    fn a_written_claim_waits_whatever_its_source_and_needs_a_page_that_was_read() {
        let mut sheet = ProductSheet::empty(ProductId::new(), at(0));
        let pages = [
            page("https://www.lamp.example/orbit/", "Orbit Desk Lamp"),
            page("https://review.example/lamps?best=1", "Best lamps"),
        ];
        let taken = sheet.take_written(
            WrittenSheet {
                claims: vec![
                    written("Lights a desk", WrittenSource::Brief),
                    // The product's own site, written another way.
                    written(
                        "Dims by touch",
                        WrittenSource::Page("http://lamp.example/orbit#specs".into()),
                    ),
                    written(
                        "Picked as a best lamp",
                        WrittenSource::Page("https://review.example/lamps?best=1".into()),
                    ),
                    // A page nobody read.
                    written(
                        "Loved by millions",
                        WrittenSource::Page("https://nowhere.example/".into()),
                    ),
                    // Not a claim.
                    written("  ", WrittenSource::Brief),
                ],
                ..WrittenSheet::default()
            },
            &pages,
            "https://lamp.example/",
            at(9),
        );
        assert_eq!(
            taken,
            SheetTaken {
                claims_added: 3,
                claims_known: 0,
                claims_dropped: 2,
                claims_over: 0,
            }
        );
        let claims = sheet.claims();
        assert!(
            claims
                .iter()
                .all(|claim| claim.state() == ClaimState::Found)
        );
        assert_eq!(*claims[0].source(), ClaimSource::Brief);
        assert_eq!(
            *claims[1].source(),
            ClaimSource::ProductPage {
                url: "https://www.lamp.example/orbit/".into()
            }
        );
        assert_eq!(
            *claims[2].source(),
            ClaimSource::Web {
                url: "https://review.example/lamps?best=1".into(),
                title: "Best lamps".into()
            }
        );
        // The pages are the sheet's sources, and it was researched.
        assert_eq!(sheet.sources().len(), 2);
        assert_eq!(sheet.sources()[0].read_at, at(5));
        assert_eq!(sheet.researched_at, Some(at(9)));
        assert_eq!(sheet.waiting(), 3);
        assert_eq!(sheet.confirmed().count(), 0);
    }

    #[test]
    fn a_sheet_written_from_the_brief_alone_takes_no_page_claims() {
        let mut sheet = ProductSheet::empty(ProductId::new(), at(0));
        let taken = sheet.take_written(
            WrittenSheet {
                claims: vec![
                    written("Lights a desk", WrittenSource::Brief),
                    written(
                        "Dims by touch",
                        WrittenSource::Page("https://lamp.example/".into()),
                    ),
                ],
                ..WrittenSheet::default()
            },
            &[],
            "https://lamp.example/",
            at(9),
        );
        assert_eq!((taken.claims_added, taken.claims_dropped), (1, 1));
        assert_eq!(sheet.researched_at, None);
        assert!(sheet.sources().is_empty());
    }

    #[test]
    fn researching_again_adds_what_is_new_and_changes_nothing_else() {
        let mut sheet = ProductSheet::empty(ProductId::new(), at(0));
        sheet.take_facts(&["Made of steel".to_owned()], at(1));
        let first = WrittenSheet {
            text: SheetText {
                summary: "A desk lamp.".into(),
                category: "Lighting".into(),
                segments: vec![AudienceSegment {
                    name: "Students".into(),
                    wants: "Light to read by".into(),
                    holds_back: "The price".into(),
                }],
                ..SheetText::default()
            },
            claims: vec![
                written("Lights a desk", WrittenSource::Brief),
                written("Dims by touch", WrittenSource::Brief),
                written("Folds flat", WrittenSource::Brief),
            ],
        };
        sheet.take_written(first, &[], "", at(2));
        let ids: Vec<ClaimId> = sheet.claims().iter().map(|claim| claim.id).collect();
        sheet
            .change_claim(ids[1], at(3), |claim| claim.confirm(at(3)))
            .unwrap();
        sheet.change_claim(ids[2], at(3), Claim::reject).unwrap();
        let before: Vec<Claim> = sheet.claims().to_vec();

        let again = WrittenSheet {
            text: SheetText {
                summary: "Another summary.".into(),
                category: "Lamps".into(),
                restricted: Some(RestrictedCategory::Health),
                segments: vec![
                    AudienceSegment {
                        name: "students".into(),
                        wants: "Something else".into(),
                        holds_back: String::new(),
                    },
                    AudienceSegment {
                        name: "Designers".into(),
                        wants: "A steady light".into(),
                        holds_back: String::new(),
                    },
                ],
                objections: vec![Objection {
                    objection: "It is small".into(),
                    answer: "It lights a whole desk".into(),
                }],
                ..SheetText::default()
            },
            claims: vec![
                // Known in every state, in other letters.
                written("made of steel", WrittenSource::Brief),
                written("LIGHTS A DESK.", WrittenSource::Brief),
                written("Dims by touch", WrittenSource::Brief),
                written("Folds flat", WrittenSource::Brief),
                written("Runs on USB", WrittenSource::Brief),
            ],
        };
        let taken = sheet.take_written(again, &[page("https://lamp.example", "Orbit")], "", at(4));
        assert_eq!(
            taken,
            SheetTaken {
                claims_added: 1,
                claims_known: 4,
                claims_dropped: 0,
                claims_over: 0,
            }
        );
        // What was there is as it was, in its place.
        assert_eq!(&sheet.claims()[..4], &before[..]);
        assert_eq!(sheet.claims()[4].text(), "Runs on USB");
        assert_eq!(sheet.claims()[4].state(), ClaimState::Found);
        // Text the sheet had stays; rows are added by name.
        assert_eq!(sheet.text().summary, "A desk lamp.");
        assert_eq!(sheet.text().category, "Lighting");
        assert_eq!(sheet.text().restricted, Some(RestrictedCategory::Health));
        let names: Vec<&str> = sheet
            .text()
            .segments
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(names, ["Students", "Designers"]);
        assert_eq!(sheet.text().segments[0].wants, "Light to read by");
        assert_eq!(sheet.text().objections.len(), 1);
    }

    #[test]
    fn a_models_text_is_made_to_fit_and_the_users_is_checked() {
        let long = "x".repeat(SheetText::MAX_SUMMARY_CHARS + 50);
        let mut segments: Vec<AudienceSegment> = (0..SheetText::MAX_SEGMENTS + 2)
            .map(|n| AudienceSegment {
                name: format!("Segment {n}"),
                ..AudienceSegment::default()
            })
            .collect();
        segments.insert(
            0,
            AudienceSegment {
                name: String::new(),
                wants: "No name".into(),
                holds_back: String::new(),
            },
        );
        let text = SheetText {
            summary: long.clone(),
            segments: segments.clone(),
            ..SheetText::default()
        };
        let fitted = text.clone().fitted();
        assert_eq!(fitted.summary.chars().count(), SheetText::MAX_SUMMARY_CHARS);
        assert_eq!(fitted.segments.len(), SheetText::MAX_SEGMENTS);
        assert_eq!(fitted.segments[0].name, "Segment 0");

        assert_eq!(
            text.validate(),
            Err(vec![
                SheetFieldError::SummaryTooLong,
                SheetFieldError::TooManySegments,
                SheetFieldError::Segment,
            ])
        );
    }

    #[test]
    fn the_users_text_is_tidied_and_empty_rows_go() {
        let mut sheet = ProductSheet::empty(ProductId::new(), at(0));
        sheet
            .set_text(
                SheetText {
                    summary: "  A desk lamp.\n\nIt dims.  ".into(),
                    category: " Home \n lighting ".into(),
                    segments: vec![AudienceSegment::default()],
                    objections: vec![
                        Objection {
                            objection: " It is small ".into(),
                            answer: String::new(),
                        },
                        Objection::default(),
                    ],
                    alternatives: vec![Alternative {
                        name: " A ceiling light ".into(),
                        differs: " Lights the desk, not the room ".into(),
                    }],
                    ..SheetText::default()
                },
                at(7),
            )
            .unwrap();
        let text = sheet.text();
        assert_eq!(text.summary, "A desk lamp.\n\nIt dims.");
        assert_eq!(text.category, "Home lighting");
        assert!(text.segments.is_empty());
        assert_eq!(text.objections.len(), 1);
        assert_eq!(text.objections[0].objection, "It is small");
        assert_eq!(text.alternatives[0].name, "A ceiling light");
        assert_eq!(sheet.updated_at, at(7));

        // A row with text needs its first part.
        let errors = sheet.set_text(
            SheetText {
                alternatives: vec![Alternative {
                    name: String::new(),
                    differs: "Cheaper".into(),
                }],
                ..SheetText::default()
            },
            at(8),
        );
        assert_eq!(errors, Err(vec![SheetFieldError::Alternative]));
        assert_eq!(sheet.updated_at, at(7));
    }

    #[test]
    fn the_user_adds_and_changes_claims_and_two_never_say_the_same() {
        let mut sheet = ProductSheet::empty(ProductId::new(), at(0));
        let id = sheet
            .add_claim("Made of steel", ClaimKind::Feature, at(1))
            .unwrap();
        assert_eq!(sheet.claim(id).unwrap().state(), ClaimState::Confirmed);
        assert_eq!(
            sheet.add_claim("made of steel.", ClaimKind::Feature, at(2)),
            Err(SheetClaimError::Known)
        );
        assert_eq!(
            sheet.add_claim("", ClaimKind::Feature, at(2)),
            Err(SheetClaimError::Claim(ClaimError::TextRequired))
        );
        let other = sheet
            .add_claim("Dims by touch", ClaimKind::Feature, at(3))
            .unwrap();
        assert_eq!(
            sheet.change_claim(other, at(4), |claim| claim.edit("Made of steel", at(4))),
            Err(SheetClaimError::Known)
        );
        assert_eq!(sheet.claim(other).unwrap().text(), "Dims by touch");
        // A change that fails leaves the claim and the sheet's time alone.
        assert_eq!(
            sheet.change_claim(other, at(5), |claim| claim.confirm(at(5))),
            Err(SheetClaimError::Claim(ClaimError::NotFound))
        );
        assert_eq!(sheet.updated_at, at(3));
        assert_eq!(
            sheet.change_claim(ClaimId::new(), at(6), Claim::reject),
            Err(SheetClaimError::Gone)
        );
        sheet.change_claim(other, at(7), Claim::reject).unwrap();
        assert_eq!(sheet.confirmed().count(), 1);
    }

    #[test]
    fn a_copied_sheet_belongs_to_the_other_product_under_new_claim_ids() {
        let mut sheet = ProductSheet::empty(ProductId::new(), at(0));
        let id = sheet
            .add_claim("Made of steel", ClaimKind::Feature, at(1))
            .unwrap();
        let other = ProductId::new();
        let copy = sheet.for_product(other, at(9));
        assert_eq!(copy.product, other);
        assert_eq!(copy.claims().len(), 1);
        assert_ne!(copy.claims()[0].id, id);
        assert_eq!(copy.claims()[0].text(), "Made of steel");
        assert_eq!(copy.claims()[0].state(), ClaimState::Confirmed);
    }

    #[test]
    fn a_fact_typed_after_a_model_said_it_makes_the_claim_the_users_own() {
        let mut sheet = ProductSheet::empty(ProductId::new(), at(0));
        sheet.take_written(
            WrittenSheet {
                claims: vec![
                    written("Dims by touch", WrittenSource::Brief),
                    written("Folds flat", WrittenSource::Brief),
                    written("Runs on USB", WrittenSource::Brief),
                ],
                ..WrittenSheet::default()
            },
            &[],
            "",
            at(1),
        );
        let ids: Vec<ClaimId> = sheet.claims().iter().map(|claim| claim.id).collect();
        sheet.change_claim(ids[1], at(2), Claim::reject).unwrap();

        // One waited, one was rejected: typed in the brief, both are the
        // user's and confirmed, in their places.
        let facts = vec!["dims by touch.".to_owned(), "Folds flat".to_owned()];
        assert_eq!(sheet.take_facts(&facts, at(3)), 2);
        for id in &ids[..2] {
            let claim = sheet.claim(*id).unwrap();
            assert_eq!(*claim.source(), ClaimSource::Typed);
            assert_eq!(claim.state(), ClaimState::Confirmed);
            assert_eq!(claim.confirmed_at(), Some(at(3)));
        }
        assert_eq!(sheet.claims()[0].text(), "Dims by touch", "its own wording");
        assert_eq!(sheet.claim(ids[2]).unwrap().state(), ClaimState::Found);
        assert_eq!(sheet.claims().len(), 3);

        // A typed claim the user then rejects stays rejected while the
        // fact is in the brief.
        sheet.change_claim(ids[0], at(4), Claim::reject).unwrap();
        assert_eq!(sheet.take_facts(&facts, at(5)), 0);
        assert_eq!(sheet.claim(ids[0]).unwrap().state(), ClaimState::Rejected);
    }

    #[test]
    fn only_the_brief_links_own_address_is_the_products_page() {
        // A link to one page of a shop: the shop's other pages are not
        // the product's.
        let link = "https://shop.example/lamps/orbit";
        for (page, own) in [
            ("https://www.shop.example/lamps/orbit/", true),
            ("https://shop.example/lamps/orbit?color=black", true),
            ("https://shop.example/lamps/orbit/specs", true),
            ("https://shop.example/lamps/orbital", false),
            ("https://shop.example/lamps/rival", false),
            ("https://shop.example/", false),
            ("https://other.example/lamps/orbit", false),
        ] {
            assert_eq!(is_product_page(link, page), own, "{page}");
        }
        // A link that is the whole site makes its pages the product's.
        assert!(is_product_page(
            "lamp.example",
            "https://www.lamp.example/faq"
        ));
        assert!(!is_product_page(
            "lamp.example",
            "https://notlamp.example/faq"
        ));
        // A link with a query names one page.
        let query = "https://shop.example/item?id=5";
        assert!(is_product_page(query, "https://shop.example/item?id=5"));
        assert!(!is_product_page(query, "https://shop.example/item?id=9"));
        assert!(!is_product_page("", "https://shop.example/"));

        let mut sheet = ProductSheet::empty(ProductId::new(), at(0));
        let pages = [
            page("https://shop.example/lamps/orbit", "Orbit"),
            page("https://shop.example/lamps/rival", "Rival"),
        ];
        sheet.take_written(
            WrittenSheet {
                claims: vec![
                    written(
                        "Dims by touch",
                        WrittenSource::Page("https://shop.example/lamps/orbit".into()),
                    ),
                    written(
                        "Rival lamps do not dim",
                        WrittenSource::Page("https://shop.example/lamps/rival".into()),
                    ),
                ],
                ..WrittenSheet::default()
            },
            &pages,
            link,
            at(1),
        );
        assert!(matches!(
            sheet.claims()[0].source(),
            ClaimSource::ProductPage { .. }
        ));
        assert_eq!(
            *sheet.claims()[1].source(),
            ClaimSource::Web {
                url: "https://shop.example/lamps/rival".into(),
                title: "Rival".into()
            }
        );
    }

    #[test]
    fn the_category_the_user_chose_stays_whatever_a_later_run_finds() {
        let health = || WrittenSheet {
            text: SheetText {
                restricted: Some(RestrictedCategory::Health),
                ..SheetText::default()
            },
            ..WrittenSheet::default()
        };
        // Saving the text with the category as it was is no choice.
        let mut sheet = ProductSheet::empty(ProductId::new(), at(0));
        sheet
            .set_text(
                SheetText {
                    summary: "A desk lamp.".into(),
                    ..SheetText::default()
                },
                at(1),
            )
            .unwrap();
        assert!(!sheet.restricted_chosen());
        sheet.take_written(health(), &[], "", at(2));
        assert_eq!(sheet.text().restricted, Some(RestrictedCategory::Health));

        // The user says it is in none: a later run does not say it again.
        let mut text = sheet.text().clone();
        text.restricted = None;
        sheet.set_text(text, at(3)).unwrap();
        assert!(sheet.restricted_chosen());
        sheet.take_written(health(), &[], "", at(4));
        assert_eq!(sheet.text().restricted, None);
        // And a copy of the sheet keeps the choice.
        assert!(
            sheet
                .for_product(ProductId::new(), at(5))
                .restricted_chosen()
        );
    }

    #[test]
    fn a_rejected_claim_takes_no_place_among_the_claims_a_sheet_holds() {
        let mut sheet = ProductSheet::empty(ProductId::new(), at(0));
        let claims = |from: usize, to: usize| WrittenSheet {
            claims: (from..to)
                .map(|n| written(&format!("Claim number {n}"), WrittenSource::Brief))
                .collect(),
            ..WrittenSheet::default()
        };
        let taken = sheet.take_written(claims(0, ProductSheet::MAX_CLAIMS + 3), &[], "", at(1));
        assert_eq!(taken.claims_added, ProductSheet::MAX_CLAIMS);
        assert_eq!((taken.claims_over, taken.claims_dropped), (3, 0));
        assert_eq!(
            sheet.add_claim("One more", ClaimKind::Feature, at(2)),
            Err(SheetClaimError::TooMany)
        );
        assert_eq!(sheet.take_facts(&["A typed fact".to_owned()], at(2)), 0);

        // Rejecting two makes room for two, and the rejected ones stay.
        let ids: Vec<ClaimId> = sheet.claims().iter().map(|claim| claim.id).collect();
        sheet.change_claim(ids[0], at(3), Claim::reject).unwrap();
        sheet.change_claim(ids[1], at(3), Claim::reject).unwrap();
        sheet
            .add_claim("One more", ClaimKind::Feature, at(4))
            .unwrap();
        let again = sheet.take_written(claims(0, ProductSheet::MAX_CLAIMS + 3), &[], "", at(5));
        assert_eq!(again.claims_known, ProductSheet::MAX_CLAIMS);
        assert_eq!((again.claims_added, again.claims_over), (1, 2));
        assert_eq!(sheet.claims().len(), ProductSheet::MAX_CLAIMS + 2);

        // A rejected claim comes back only where there is a place.
        assert_eq!(
            sheet.change_claim(ids[0], at(6), |claim| claim.bring_back(at(6))),
            Err(SheetClaimError::TooMany)
        );
        sheet.change_claim(ids[2], at(7), Claim::reject).unwrap();
        sheet
            .change_claim(ids[0], at(8), |claim| claim.bring_back(at(8)))
            .unwrap();
    }
}
