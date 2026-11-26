//! What an ad's post is reviewed with (docs/spec/marketing-videos.md,
//! "Publishing an ad"; ADR-0045): its commercial disclosure on each
//! network, and the items the user acknowledges before an upload or an
//! export of it starts.
//!
//! Bardo sets defaults and reminds; the user decides each disclosure.
//! Projects that are not ads have neither.

use crate::{CheckedScript, Network, ProductOwnership, RestrictedCategory, StyleChoice};

/// Whether a post promotes something, and whose: apart from the
/// synthetic-content disclosure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CommercialDisclosure {
    /// Nothing to disclose.
    None,
    /// The user's own brand or business.
    #[default]
    YourBrand,
    /// Someone else's product, promoted for them.
    PaidPartnership,
}

impl CommercialDisclosure {
    pub const ALL: [CommercialDisclosure; 3] = [
        CommercialDisclosure::None,
        CommercialDisclosure::YourBrand,
        CommercialDisclosure::PaidPartnership,
    ];

    /// The stable code it is stored under.
    pub fn code(self) -> &'static str {
        match self {
            CommercialDisclosure::None => "none",
            CommercialDisclosure::YourBrand => "your_brand",
            CommercialDisclosure::PaidPartnership => "paid_partnership",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|disclosure| disclosure.code() == code)
    }

    /// The default of an ad for a product of `owner`: the user's own
    /// brand, or a paid partnership for a client's product.
    pub fn default_for(owner: ProductOwnership) -> Self {
        match owner {
            ProductOwnership::Mine => CommercialDisclosure::YourBrand,
            ProductOwnership::Client => CommercialDisclosure::PaidPartnership,
        }
    }

    /// Whether the post carries a paid-partnership label.
    pub fn is_paid(self) -> bool {
        self == CommercialDisclosure::PaidPartnership
    }
}

/// What disclosing takes on one network, as each network documents it
/// (checked on 2026-11-26).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisclosureStep {
    /// Nothing to set: the network has no label for it.
    Nothing,
    /// Bardo sends the label with the post (Instagram's
    /// `is_paid_partnership`, X's `paid_partnership`).
    Sent,
    /// The user ticks *Includes paid promotion* in YouTube Studio: the
    /// API reads the box but no upload can set it.
    InStudio,
    /// The user turns on *Disclose post content* in TikTok before posting
    /// the draft: the inbox upload has no disclosure field.
    InApp,
}

/// What disclosing `disclosure` takes on `network`. Kick takes no upload;
/// its export, as every export, carries a reminder line.
pub fn disclosure_step(network: Network, disclosure: CommercialDisclosure) -> DisclosureStep {
    match (disclosure, network) {
        (CommercialDisclosure::None, _) => DisclosureStep::Nothing,
        (_, Network::TikTok) => DisclosureStep::InApp,
        (CommercialDisclosure::YourBrand, _) => DisclosureStep::Nothing,
        (CommercialDisclosure::PaidPartnership, Network::YouTube) => DisclosureStep::InStudio,
        (CommercialDisclosure::PaidPartnership, Network::InstagramReels | Network::X) => {
            DisclosureStep::Sent
        }
        (CommercialDisclosure::PaidPartnership, Network::Kick) => DisclosureStep::Nothing,
    }
}

/// Why typed brand accounts cannot go with a Reel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum SponsorError {
    /// An entry is not an Instagram account id: digits only.
    #[error("a brand account is not an Instagram account id")]
    NotAnId,
    /// More than [`MAX_SPONSORS`].
    #[error("too many brand accounts")]
    TooMany,
}

/// The brand accounts a paid-partnership Reel may name (Meta's limit).
pub const MAX_SPONSORS: usize = 2;

/// The Instagram account ids the user typed for a paid-partnership Reel,
/// separated by commas or spaces, each once. Empty is fine: the label
/// goes without a brand.
pub fn sponsor_ids(typed: &str) -> Result<Vec<String>, SponsorError> {
    let mut ids: Vec<String> = Vec::new();
    for entry in typed
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|entry| !entry.is_empty())
    {
        if entry.len() > 20 || !entry.chars().all(|c| c.is_ascii_digit()) {
            return Err(SponsorError::NotAnId);
        }
        if !ids.iter().any(|id| id == entry) {
            ids.push(entry.to_owned());
        }
    }
    if ids.len() > MAX_SPONSORS {
        return Err(SponsorError::TooMany);
    }
    Ok(ids)
}

/// Something about an ad the user acknowledges before an upload or an
/// export of it starts.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AdReviewItem {
    /// A claim the script or the post texts use that the product marks as
    /// sensitive, as the video states it.
    SensitiveClaim(String),
    /// A statement the claims check found no claim backing.
    Unbacked { of: CheckedScript, says: String },
    /// The text was not checked as it reads now: never, or before an edit
    /// or a change to the brief.
    NotChecked(CheckedScript),
    /// A claim the video uses that its product no longer confirms, as the
    /// video states it.
    Unconfirmed(String),
    /// The product is in a category the networks have their own rules for.
    RestrictedCategory(RestrictedCategory),
    /// The video shows an idea, not a product that exists.
    Concept,
}

impl AdReviewItem {
    /// What an acknowledgement names: the same item keeps its key, and a
    /// changed one needs a new acknowledgement.
    /// One line: what the item names has its whitespace folded.
    pub fn key(&self) -> String {
        let key = match self {
            AdReviewItem::SensitiveClaim(text) => format!("sensitive:{text}"),
            AdReviewItem::Unbacked { of, says } => format!("unbacked:{}:{says}", of.code()),
            AdReviewItem::NotChecked(of) => format!("not_checked:{}", of.code()),
            AdReviewItem::Unconfirmed(text) => format!("unconfirmed:{text}"),
            AdReviewItem::RestrictedCategory(category) => {
                format!("category:{}", category.code())
            }
            AdReviewItem::Concept => "concept".to_owned(),
        };
        key.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}

/// The items of `items` that `acknowledged` (their keys) leaves out.
pub fn unacknowledged<'a>(
    items: &'a [AdReviewItem],
    acknowledged: &[String],
) -> Vec<&'a AdReviewItem> {
    items
        .iter()
        .filter(|item| !acknowledged.contains(&item.key()))
        .collect()
}

/// What an ad's upload or export was reviewed with: kept with the
/// publication or the export.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AdPostReview {
    pub disclosure: CommercialDisclosure,
    /// The brand accounts a paid-partnership Reel names.
    pub sponsors: Vec<String>,
    /// The keys of the items the user acknowledged.
    pub acknowledged: Vec<String>,
}

/// Whether an ad's posts start with the synthetic-content disclosure on:
/// a *product in use* scene drawn in a photographic style, or in a style
/// Bardo cannot tell (the channel's own notes), is a realistic picture of
/// a real thing that was not filmed.
pub fn ad_synthetic_default(has_product_in_use: bool, style: StyleChoice) -> bool {
    has_product_in_use
        && match style {
            StyleChoice::FromNotes => true,
            StyleChoice::Catalog(style) => style.is_photographic(),
        }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VisualStyle;

    #[test]
    fn the_default_follows_whose_the_product_is() {
        assert_eq!(
            CommercialDisclosure::default_for(ProductOwnership::Mine),
            CommercialDisclosure::YourBrand
        );
        assert_eq!(
            CommercialDisclosure::default_for(ProductOwnership::Client),
            CommercialDisclosure::PaidPartnership
        );
        for disclosure in CommercialDisclosure::ALL {
            assert_eq!(
                CommercialDisclosure::from_code(disclosure.code()),
                Some(disclosure)
            );
        }
        assert_eq!(CommercialDisclosure::from_code("paid"), None);
    }

    #[test]
    fn each_network_takes_what_it_documents() {
        use CommercialDisclosure::*;
        use DisclosureStep::*;
        let step = disclosure_step;
        for network in Network::ALL {
            assert_eq!(step(network, None), Nothing, "{network:?}");
        }
        assert_eq!(step(Network::YouTube, YourBrand), Nothing);
        assert_eq!(step(Network::YouTube, PaidPartnership), InStudio);
        assert_eq!(step(Network::TikTok, YourBrand), InApp);
        assert_eq!(step(Network::TikTok, PaidPartnership), InApp);
        assert_eq!(step(Network::InstagramReels, YourBrand), Nothing);
        assert_eq!(step(Network::InstagramReels, PaidPartnership), Sent);
        assert_eq!(step(Network::X, YourBrand), Nothing);
        assert_eq!(step(Network::X, PaidPartnership), Sent);
        assert_eq!(step(Network::Kick, PaidPartnership), Nothing);
    }

    #[test]
    fn brand_accounts_are_up_to_two_instagram_ids() {
        assert_eq!(sponsor_ids(""), Ok(vec![]));
        assert_eq!(
            sponsor_ids(" 1784, 9001 1784 "),
            Ok(vec!["1784".to_owned(), "9001".to_owned()])
        );
        assert_eq!(sponsor_ids("@brand"), Err(SponsorError::NotAnId));
        assert_eq!(sponsor_ids("1 2 3"), Err(SponsorError::TooMany));
        assert_eq!(
            sponsor_ids("123456789012345678901"),
            Err(SponsorError::NotAnId)
        );
    }

    #[test]
    fn an_item_is_acknowledged_by_its_key_and_a_changed_one_is_not() {
        let items = vec![
            AdReviewItem::SensitiveClaim("Lasts 40 hours".to_owned()),
            AdReviewItem::Unbacked {
                of: CheckedScript::Posts,
                says: "the best lamp".to_owned(),
            },
            AdReviewItem::NotChecked(CheckedScript::Current),
            AdReviewItem::Unconfirmed("Folds flat".to_owned()),
            AdReviewItem::RestrictedCategory(RestrictedCategory::Health),
            AdReviewItem::Concept,
        ];
        let keys: Vec<String> = items.iter().map(AdReviewItem::key).collect();
        let mut unique = keys.clone();
        unique.dedup();
        assert_eq!(unique.len(), items.len());
        assert!(unacknowledged(&items, &keys).is_empty());
        let mut some = keys.clone();
        some.remove(0);
        assert_eq!(unacknowledged(&items, &some), vec![&items[0]]);
        let edited = [AdReviewItem::SensitiveClaim("Lasts 50 hours".to_owned())];
        assert_eq!(unacknowledged(&edited, &keys).len(), 1);
    }

    #[test]
    fn the_synthetic_default_follows_the_scene_kinds_and_the_style() {
        assert!(!ad_synthetic_default(false, StyleChoice::FromNotes));
        assert!(ad_synthetic_default(true, StyleChoice::FromNotes));
        assert!(ad_synthetic_default(
            true,
            StyleChoice::Catalog(VisualStyle::Photorealistic)
        ));
        assert!(ad_synthetic_default(
            true,
            StyleChoice::Catalog(VisualStyle::DocumentaryPhoto)
        ));
        assert!(!ad_synthetic_default(
            true,
            StyleChoice::Catalog(VisualStyle::Watercolor)
        ));
        assert!(!ad_synthetic_default(
            false,
            StyleChoice::Catalog(VisualStyle::Photorealistic)
        ));
    }
}
