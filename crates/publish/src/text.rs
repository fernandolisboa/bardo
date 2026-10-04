//! A network's own words, as Bardo keeps and shows them.

/// The most of a network's own words Bardo keeps and shows.
pub(crate) const SHOWN_TEXT: usize = 500;

/// A network's text as one plain line: no control or direction-changing
/// characters (which could make a notice read as something else), trimmed,
/// at most `max` characters.
pub(crate) fn plain(text: &str, max: usize) -> String {
    let kept: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .filter(|c| {
            !matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .collect();
    kept.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max)
        .collect()
}
