//! The keyboard shortcuts the Guide lists, by where they work. Key names
//! are the keyboard's own and read the same in every language.

use crate::Text;

/// One shortcut: the keys (any of them works) and what they do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    pub keys: &'static [&'static str],
    pub action: Text,
}

/// Shortcuts that work in the same place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShortcutGroup {
    pub name: Text,
    pub shortcuts: &'static [Shortcut],
}

const fn shortcut(keys: &'static [&'static str], action: &'static str) -> Shortcut {
    Shortcut {
        keys,
        action: Text::ShortcutAction(action),
    }
}

/// Every shortcut the app has, in the order the Guide lists them.
pub const SHORTCUTS: &[ShortcutGroup] = &[
    ShortcutGroup {
        name: Text::ShortcutGroup("guide"),
        shortcuts: &[
            shortcut(&["F1"], "guide_open"),
            shortcut(&["↑", "↓"], "guide_move"),
            shortcut(&["Enter"], "guide_pick"),
            shortcut(&["Esc"], "guide_clear"),
        ],
    },
    ShortcutGroup {
        name: Text::ShortcutGroup("tour"),
        shortcuts: &[
            shortcut(&["→", "Enter"], "tour_next"),
            shortcut(&["←"], "tour_back"),
            shortcut(&["Esc"], "tour_close"),
            shortcut(&["Tab", "Shift+Tab"], "tour_buttons"),
        ],
    },
    ShortcutGroup {
        name: Text::ShortcutGroup("lists"),
        shortcuts: &[
            shortcut(&["↑", "↓"], "list_move"),
            shortcut(&["Enter"], "list_accept"),
        ],
    },
    ShortcutGroup {
        name: Text::ShortcutGroup("editor"),
        shortcuts: &[
            shortcut(&["Space"], "play"),
            shortcut(&["←", "→"], "frame"),
            shortcut(&["Home", "End"], "start_end"),
            shortcut(&["S"], "split"),
            shortcut(&["[", "]"], "trim"),
            shortcut(&["Delete"], "delete"),
            shortcut(&["Alt+←", "Alt+→"], "nudge"),
            shortcut(&["Esc"], "deselect"),
            shortcut(&["Ctrl+Z"], "undo"),
            shortcut(&["Ctrl+Y", "Ctrl+Shift+Z"], "redo"),
        ],
    },
    ShortcutGroup {
        name: Text::ShortcutGroup("suggestions"),
        shortcuts: &[
            shortcut(&["Tab"], "next_cut"),
            shortcut(&["A"], "accept_cut"),
            shortcut(&["R"], "reject_cut"),
        ],
    },
];
