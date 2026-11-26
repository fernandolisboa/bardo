//! What an ad's upload or export was reviewed with (#282), as its rows
//! keep it: lists one entry per line.

use bardo_domain::CommercialDisclosure;

/// The entries of a list kept one per line.
pub(crate) fn lines_of(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The disclosure stored under `code`. One Bardo does not know (a newer
/// version's) reads as a paid partnership: the label errs on the side of
/// being shown.
pub(crate) fn disclosure_of(code: &str) -> CommercialDisclosure {
    CommercialDisclosure::from_code(code).unwrap_or(CommercialDisclosure::PaidPartnership)
}
