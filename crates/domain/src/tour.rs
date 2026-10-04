//! Guided tours' progress (issue #105): which tours a profile was offered,
//! where it left one, and which content version it saw. What a tour says
//! and points at lives in the app; only the user's progress is kept.

use std::sync::Arc;
use std::time::SystemTime;

use crate::{ProfileId, RepositoryError};

/// A guided tour, as its progress is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TourId {
    /// The first-run tour of the navigation and the first steps.
    Welcome,
    /// The Research screen's tour.
    Research,
    /// The Themes screen's tour.
    Themes,
    /// The Performance screen's tour.
    Performance,
    /// The Projects screen's tour: the project, its narrator and stages.
    Projects,
    /// The Script stage's tour.
    Script,
    /// The Narration stage's tour.
    Narration,
    /// The Scenes stage's tour.
    Scenes,
    /// The Clips stage's tour.
    Clips,
    /// The Personas screen's tour.
    Personas,
    /// The Templates screen's tour.
    Templates,
}

impl TourId {
    pub const ALL: [TourId; 11] = [
        TourId::Welcome,
        TourId::Research,
        TourId::Themes,
        TourId::Performance,
        TourId::Projects,
        TourId::Script,
        TourId::Narration,
        TourId::Scenes,
        TourId::Clips,
        TourId::Personas,
        TourId::Templates,
    ];

    pub fn code(self) -> &'static str {
        match self {
            TourId::Welcome => "welcome",
            TourId::Research => "research",
            TourId::Themes => "themes",
            TourId::Performance => "performance",
            TourId::Projects => "projects",
            TourId::Script => "script",
            TourId::Narration => "narration",
            TourId::Scenes => "scenes",
            TourId::Clips => "clips",
            TourId::Personas => "personas",
            TourId::Templates => "templates",
        }
    }

    /// The tour stored as `code`; `None` for one this version does not know
    /// (removed later, or from a newer version), whose progress is ignored.
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tour| tour.code() == code)
    }
}

/// Where a profile stands with one tour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TourState {
    /// Offered and put off with "Not now": offered again next time.
    Offered,
    /// Started and not finished; "Resume tour" goes back to its last step.
    InProgress,
    /// Finished.
    Completed,
    /// Skipped, or "Don't show again": never offered on its own again.
    Dismissed,
}

impl TourState {
    pub const ALL: [TourState; 4] = [
        TourState::Offered,
        TourState::InProgress,
        TourState::Completed,
        TourState::Dismissed,
    ];

    pub fn code(self) -> &'static str {
        match self {
            TourState::Offered => "offered",
            TourState::InProgress => "in_progress",
            TourState::Completed => "completed",
            TourState::Dismissed => "dismissed",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|state| state.code() == code)
    }
}

/// One profile's progress through one tour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TourProgress {
    pub profile: ProfileId,
    pub tour: TourId,
    /// The tour's content version when this was saved. A tour whose
    /// content version is higher reads as new again; it never restarts on
    /// its own.
    pub version: u32,
    pub state: TourState,
    /// The step shown last, from 0.
    pub last_step: u32,
    pub updated_at: SystemTime,
}

/// Where tour progress is kept, per profile.
pub trait TourProgressRepository {
    /// Every tour the profile has progress in, in no order.
    fn tour_progress(&self, profile: ProfileId) -> Result<Vec<TourProgress>, RepositoryError>;

    /// Inserts or replaces the progress of `progress.tour`.
    fn save_tour_progress(&self, progress: &TourProgress) -> Result<(), RepositoryError>;

    /// Forgets every tour's progress for the profile ("Reset tours").
    fn reset_tour_progress(&self, profile: ProfileId) -> Result<(), RepositoryError>;
}

impl<T: TourProgressRepository + ?Sized> TourProgressRepository for Arc<T> {
    fn tour_progress(&self, profile: ProfileId) -> Result<Vec<TourProgress>, RepositoryError> {
        (**self).tour_progress(profile)
    }

    fn save_tour_progress(&self, progress: &TourProgress) -> Result<(), RepositoryError> {
        (**self).save_tour_progress(progress)
    }

    fn reset_tour_progress(&self, profile: ProfileId) -> Result<(), RepositoryError> {
        (**self).reset_tour_progress(profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip() {
        for tour in TourId::ALL {
            assert_eq!(TourId::from_code(tour.code()), Some(tour));
        }
        for state in TourState::ALL {
            assert_eq!(TourState::from_code(state.code()), Some(state));
        }
        assert_eq!(TourId::from_code("editor-2030"), None);
        assert_eq!(TourState::from_code("paused"), None);
    }
}
