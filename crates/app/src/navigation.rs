//! Where the user can go, grouped as the product is: the open project's
//! places, then the global library; jobs, costs, the guide and settings
//! sit apart. Every layout shows the same places in this order; only where
//! it draws them changes.

/// A part of the navigation, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NavSection {
    /// The open project's places.
    Project,
    /// What every project can use.
    Library,
}

impl NavSection {
    pub const ALL: [NavSection; 2] = [NavSection::Project, NavSection::Library];
}

/// A place the navigation leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Destination {
    /// The open project at a glance: its videos in progress with their
    /// next steps, the ideas waiting and its channels.
    Overview,
    Research,
    Themes,
    /// The project's videos and their stages.
    Videos,
    /// The narrators (personas) that speak the videos.
    Narrators,
    /// The products to advertise, with their briefs and pictures.
    Products,
    Templates,
    Channels,
    Accounts,
    Performance,
    /// The global library: narrators, products and templates every
    /// project can use, under tabs.
    Library,
    Jobs,
    Costs,
    /// Help: the tours, the shortcuts and the user guide.
    Guide,
    Settings,
}

/// An entry of the navigation: one place, or places shown under tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NavEntry {
    Overview,
    /// Research and Themes.
    Ideas,
    Videos,
    Narrators,
    Products,
    Templates,
    /// Channels and network accounts.
    ChannelsAndAccounts,
    Performance,
    Library,
}

impl NavEntry {
    /// Its places, the first one opened when nothing else was.
    pub fn places(self) -> &'static [Destination] {
        match self {
            NavEntry::Overview => &[Destination::Overview],
            NavEntry::Ideas => &[Destination::Research, Destination::Themes],
            NavEntry::Videos => &[Destination::Videos],
            NavEntry::Narrators => &[Destination::Narrators],
            NavEntry::Products => &[Destination::Products],
            NavEntry::Templates => &[Destination::Templates],
            NavEntry::ChannelsAndAccounts => &[Destination::Channels, Destination::Accounts],
            NavEntry::Performance => &[Destination::Performance],
            NavEntry::Library => &[Destination::Library],
        }
    }

    /// Whether its places show under tabs.
    pub fn has_tabs(self) -> bool {
        self.places().len() > 1
    }
}

/// A tab of the global library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LibraryTab {
    Narrators,
    Products,
    Templates,
}

impl LibraryTab {
    pub const ALL: [LibraryTab; 3] = [
        LibraryTab::Narrators,
        LibraryTab::Products,
        LibraryTab::Templates,
    ];

    /// The tab's name in links (`bardo:go/library/products`).
    pub fn code(self) -> &'static str {
        match self {
            LibraryTab::Narrators => "narrators",
            LibraryTab::Products => "products",
            LibraryTab::Templates => "templates",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tab| tab.code() == code)
    }

    /// The screen the tab shows: the same one the project's entry shows,
    /// until a project owns its own.
    pub fn place(self) -> Destination {
        match self {
            LibraryTab::Narrators => Destination::Narrators,
            LibraryTab::Products => Destination::Products,
            LibraryTab::Templates => Destination::Templates,
        }
    }

    /// The tab that shows `place`, if one does.
    pub fn of(place: Destination) -> Option<Self> {
        Self::ALL.into_iter().find(|tab| tab.place() == place)
    }
}

impl Destination {
    pub const ALL: [Destination; 15] = [
        Destination::Overview,
        Destination::Research,
        Destination::Themes,
        Destination::Videos,
        Destination::Narrators,
        Destination::Products,
        Destination::Templates,
        Destination::Channels,
        Destination::Accounts,
        Destination::Performance,
        Destination::Library,
        Destination::Jobs,
        Destination::Costs,
        Destination::Guide,
        Destination::Settings,
    ];

    /// The navigation's sections and their entries.
    pub const SECTIONS: [(NavSection, &'static [NavEntry]); 2] = [
        (
            NavSection::Project,
            &[
                NavEntry::Overview,
                NavEntry::Ideas,
                NavEntry::Videos,
                NavEntry::Narrators,
                NavEntry::Products,
                NavEntry::Templates,
                NavEntry::ChannelsAndAccounts,
                NavEntry::Performance,
            ],
        ),
        (NavSection::Library, &[NavEntry::Library]),
    ];

    /// The places outside the sections, always at hand.
    pub const PINNED: [Destination; 4] = [
        Destination::Jobs,
        Destination::Costs,
        Destination::Guide,
        Destination::Settings,
    ];

    /// Where the app opens: the open project's overview.
    pub const START: Destination = Destination::Overview;

    /// The entry that holds the place; `None` for the pinned ones.
    pub fn entry(self) -> Option<NavEntry> {
        Self::SECTIONS
            .iter()
            .flat_map(|(_, entries)| entries.iter())
            .find(|entry| entry.places().contains(&self))
            .copied()
    }

    /// The section the place belongs to; `None` for the pinned ones.
    pub fn section(self) -> Option<NavSection> {
        Self::SECTIONS
            .iter()
            .find(|(_, entries)| entries.iter().any(|entry| entry.places().contains(&self)))
            .map(|(section, _)| *section)
    }

    /// The place's name in links (`bardo:go/research`) and the user guide.
    pub fn code(self) -> &'static str {
        match self {
            Destination::Overview => "overview",
            Destination::Research => "research",
            Destination::Themes => "themes",
            Destination::Videos => "videos",
            Destination::Narrators => "narrators",
            Destination::Products => "products",
            Destination::Templates => "templates",
            Destination::Channels => "channels",
            Destination::Accounts => "accounts",
            Destination::Performance => "performance",
            Destination::Library => "library",
            Destination::Jobs => "jobs",
            Destination::Costs => "costs",
            Destination::Guide => "guide",
            Destination::Settings => "settings",
        }
    }

    /// The place `code` names, older names included: `projects` is now
    /// Videos and `personas` Narrators.
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "projects" => Some(Destination::Videos),
            "personas" => Some(Destination::Narrators),
            code => Self::ALL.into_iter().find(|place| place.code() == code),
        }
    }
}

/// A tab of the Settings screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingsTab {
    Keys,
    Networks,
    /// Publishing while Bardo is closed: the background agent.
    Publishing,
    Appearance,
    Metrics,
    /// Backing up and restoring Bardo's data.
    Backup,
    /// Bardo's version, the FFmpeg it ships and the third-party notices.
    About,
}

impl SettingsTab {
    pub const ALL: [SettingsTab; 7] = [
        SettingsTab::Keys,
        SettingsTab::Networks,
        SettingsTab::Publishing,
        SettingsTab::Appearance,
        SettingsTab::Metrics,
        SettingsTab::Backup,
        SettingsTab::About,
    ];

    /// The tab's name in links (`bardo:go/settings/keys`).
    pub fn code(self) -> &'static str {
        match self {
            SettingsTab::Keys => "keys",
            SettingsTab::Networks => "networks",
            SettingsTab::Publishing => "publishing",
            SettingsTab::Appearance => "appearance",
            SettingsTab::Metrics => "metrics",
            SettingsTab::Backup => "backup",
            SettingsTab::About => "about",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tab| tab.code() == code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_place_is_listed_once() {
        for place in Destination::ALL {
            let sectioned = Destination::SECTIONS
                .iter()
                .flat_map(|(_, entries)| entries.iter())
                .flat_map(|entry| entry.places().iter())
                .filter(|p| **p == place)
                .count();
            let pinned = Destination::PINNED.iter().filter(|p| **p == place).count();
            assert_eq!(sectioned + pinned, 1, "{place:?}");
        }
    }

    #[test]
    fn codes_read_back_and_old_names_still_land() {
        for place in Destination::ALL {
            assert_eq!(Destination::from_code(place.code()), Some(place));
        }
        for tab in SettingsTab::ALL {
            assert_eq!(SettingsTab::from_code(tab.code()), Some(tab));
        }
        for tab in LibraryTab::ALL {
            assert_eq!(LibraryTab::from_code(tab.code()), Some(tab));
            assert_eq!(LibraryTab::of(tab.place()), Some(tab));
        }
        assert_eq!(
            Destination::from_code("projects"),
            Some(Destination::Videos)
        );
        assert_eq!(
            Destination::from_code("personas"),
            Some(Destination::Narrators)
        );
        assert_eq!(
            Destination::from_code("research"),
            Some(Destination::Research)
        );
        assert_eq!(Destination::from_code("themes"), Some(Destination::Themes));
        assert_eq!(Destination::from_code("nowhere"), None);
    }

    #[test]
    fn the_project_comes_first_then_the_library() {
        assert_eq!(Destination::Overview.section(), Some(NavSection::Project));
        assert_eq!(Destination::Research.entry(), Some(NavEntry::Ideas));
        assert_eq!(Destination::Themes.entry(), Some(NavEntry::Ideas));
        assert_eq!(
            Destination::Accounts.entry(),
            Some(NavEntry::ChannelsAndAccounts)
        );
        assert_eq!(Destination::Library.section(), Some(NavSection::Library));
        assert_eq!(Destination::Costs.section(), None);
        assert_eq!(Destination::Guide.entry(), None);
        assert!(NavEntry::Ideas.has_tabs());
        assert!(!NavEntry::Videos.has_tabs());
        assert_eq!(
            Destination::SECTIONS.map(|(section, _)| section),
            NavSection::ALL
        );
        assert_eq!(Destination::START, Destination::Overview);
    }
}
