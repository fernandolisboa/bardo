//! Where the user can go, grouped as the product is: the pillars hold the
//! work screens; jobs, costs and settings sit apart. Every layout shows
//! the same places in this order; only where it draws them changes.

/// A pillar of the product, in priority order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pillar {
    Strategy,
    Production,
    Publishing,
}

impl Pillar {
    pub const ALL: [Pillar; 3] = [Pillar::Strategy, Pillar::Production, Pillar::Publishing];
}

/// A place the navigation leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Destination {
    Research,
    Themes,
    Projects,
    Personas,
    Templates,
    Channels,
    Accounts,
    Jobs,
    Costs,
    Settings,
}

impl Destination {
    pub const ALL: [Destination; 10] = [
        Destination::Research,
        Destination::Themes,
        Destination::Projects,
        Destination::Personas,
        Destination::Templates,
        Destination::Channels,
        Destination::Accounts,
        Destination::Jobs,
        Destination::Costs,
        Destination::Settings,
    ];

    /// The work screens, by pillar.
    pub const GROUPS: [(Pillar, &'static [Destination]); 3] = [
        (
            Pillar::Strategy,
            &[Destination::Research, Destination::Themes],
        ),
        (
            Pillar::Production,
            &[
                Destination::Projects,
                Destination::Personas,
                Destination::Templates,
            ],
        ),
        (
            Pillar::Publishing,
            &[Destination::Channels, Destination::Accounts],
        ),
    ];

    /// The places outside the pillars, always at hand.
    pub const PINNED: [Destination; 3] =
        [Destination::Jobs, Destination::Costs, Destination::Settings];

    /// Where the app opens: making videos is the daily work.
    pub const START: Destination = Destination::Projects;

    /// The pillar the place belongs to; `None` for the pinned ones.
    pub fn pillar(self) -> Option<Pillar> {
        Self::GROUPS
            .iter()
            .find(|(_, places)| places.contains(&self))
            .map(|(pillar, _)| *pillar)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_place_is_listed_once() {
        for place in Destination::ALL {
            let grouped = Destination::GROUPS
                .iter()
                .flat_map(|(_, places)| places.iter())
                .filter(|p| **p == place)
                .count();
            let pinned = Destination::PINNED.iter().filter(|p| **p == place).count();
            assert_eq!(grouped + pinned, 1, "{place:?}");
        }
    }

    #[test]
    fn the_pillars_hold_their_screens() {
        assert_eq!(Destination::Research.pillar(), Some(Pillar::Strategy));
        assert_eq!(Destination::Themes.pillar(), Some(Pillar::Strategy));
        assert_eq!(Destination::Projects.pillar(), Some(Pillar::Production));
        assert_eq!(Destination::Templates.pillar(), Some(Pillar::Production));
        assert_eq!(Destination::Accounts.pillar(), Some(Pillar::Publishing));
        assert_eq!(Destination::Costs.pillar(), None);
        assert_eq!(
            Destination::GROUPS.map(|(pillar, _)| pillar),
            [Pillar::Strategy, Pillar::Production, Pillar::Publishing],
            "in priority order"
        );
    }
}
