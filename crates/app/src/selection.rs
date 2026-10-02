//! Moving the selection through a collection from the keyboard (issue
//! #61): ↑ and ↓ walk the items on screen, and the inspector follows.

/// A move of the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Previous,
    Next,
}

/// The item a `step` from `selected` lands on, among `shown` (the indexes
/// of the items on screen, in their order). Without a selection on
/// screen, ↓ picks the first and ↑ the last; at either end it stays put.
/// `None` only when nothing is shown.
pub fn step_selection(shown: &[usize], selected: Option<usize>, step: Step) -> Option<usize> {
    let at = selected.and_then(|selected| shown.iter().position(|index| *index == selected));
    let to = match (at, step) {
        (None, Step::Next) => 0,
        (None, Step::Previous) => shown.len().checked_sub(1)?,
        (Some(at), Step::Next) => (at + 1).min(shown.len() - 1),
        (Some(at), Step::Previous) => at.saturating_sub(1),
    };
    shown.get(to).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn down_and_up_walk_the_shown_items() {
        let shown = [0, 1, 2];
        assert_eq!(step_selection(&shown, Some(0), Step::Next), Some(1));
        assert_eq!(step_selection(&shown, Some(2), Step::Previous), Some(1));
    }

    #[test]
    fn the_ends_hold() {
        let shown = [0, 1, 2];
        assert_eq!(step_selection(&shown, Some(2), Step::Next), Some(2));
        assert_eq!(step_selection(&shown, Some(0), Step::Previous), Some(0));
    }

    #[test]
    fn a_filtered_list_skips_the_hidden_items() {
        // Only the pending scenes 1, 4 and 5 are on screen.
        let shown = [1, 4, 5];
        assert_eq!(step_selection(&shown, Some(1), Step::Next), Some(4));
        assert_eq!(step_selection(&shown, Some(5), Step::Previous), Some(4));
    }

    #[test]
    fn without_a_selection_on_screen_down_takes_the_first_and_up_the_last() {
        let shown = [1, 4, 5];
        assert_eq!(step_selection(&shown, None, Step::Next), Some(1));
        assert_eq!(step_selection(&shown, None, Step::Previous), Some(5));
        // The selection was filtered out.
        assert_eq!(step_selection(&shown, Some(2), Step::Next), Some(1));
    }

    #[test]
    fn nothing_shown_selects_nothing() {
        assert_eq!(step_selection(&[], Some(0), Step::Next), None);
        assert_eq!(step_selection(&[], None, Step::Previous), None);
    }
}
