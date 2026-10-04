//! Guided tours (issue #105): the window dims, one component at a time
//! stays lit, and a card beside it says what it is and how to use it.
//!
//! A tour is a list of steps, each pointing at a [`TourAnchor`]: a name
//! for a component that does not depend on the layout, so the same step
//! lights the sidebar in Workspace and the top bar in Studio. The UI tells
//! which anchors are on screen; everything else (where the run is, what a
//! missing anchor turns into, what is saved) is decided here. Steps only
//! explain: a tour never creates, changes or deletes the user's data.

use std::collections::HashMap;
use std::time::SystemTime;

use bardo_domain::{ProfileId, TourId, TourProgress, TourProgressRepository, TourState};

use crate::{AppError, Bardo, Destination, Pillar, Stage, Text};

/// A component a tour step can point at. The layouts tag the places and
/// parts they draw, wherever they draw them; screens tag their own
/// controls as later tours need them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TourAnchor {
    /// A pillar's places in the navigation.
    NavGroup(Pillar),
    /// One place in the navigation, pinned ones included.
    NavPlace(Destination),
    /// A screen's header: where it is, its title, its actions.
    Header,
    /// The project stages.
    Stages,
    /// Actions over the collection or the page.
    Toolbar,
    /// The items a screen holds.
    Collection,
    /// The selected item's properties, or the form that drives the
    /// collection.
    Inspector,
    /// What a page shows when it is not a collection.
    Content,
}

/// A place a step opens before it shows, so its anchor is on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TourPlace {
    Screen(Destination),
    /// A stage of the open video project.
    Stage(Stage),
}

/// Where a step's card goes beside its lit component. A side without
/// room flips to the other one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The side the layout leaves open around the component: right of a
    /// sidebar, below a top bar.
    Open,
    Right,
    Left,
    Below,
    Above,
}

/// What a step does when its component is not on screen (a screen with no
/// items yet, a part the layout does not draw).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhenMissing {
    /// Go on to the next step in the direction the user was going.
    Skip,
    /// Light the part that holds the component instead.
    LightPart(TourAnchor),
    /// Show the card in the middle, with nothing lit.
    Center,
}

/// One step: what it lights, where it goes first, what its card says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TourStep {
    /// Names the step's texts: `tour.<tour>.<key>.title` and `.body`.
    pub key: &'static str,
    /// `None` for a card in the middle with nothing lit.
    pub anchor: Option<TourAnchor>,
    pub place: Option<TourPlace>,
    pub side: Side,
    pub when_missing: WhenMissing,
}

impl TourStep {
    /// A step that lights `anchor`, with its card on the open side.
    const fn at(key: &'static str, anchor: TourAnchor) -> Self {
        Self {
            key,
            anchor: Some(anchor),
            place: None,
            side: Side::Open,
            when_missing: WhenMissing::Center,
        }
    }

    /// A card in the middle with nothing lit.
    const fn centered(key: &'static str) -> Self {
        Self {
            key,
            anchor: None,
            place: None,
            side: Side::Open,
            when_missing: WhenMissing::Center,
        }
    }
}

/// A tour's steps and its content version. Raise the version when the
/// steps change enough that someone who saw them should see them again: the
/// tour shows as new, and never restarts on its own.
#[derive(Debug, PartialEq, Eq)]
pub struct Tour {
    pub id: TourId,
    pub version: u32,
    pub steps: &'static [TourStep],
}

/// The first-run tour: the pillars, the keys, jobs, costs and where help
/// lives. Every step lights the navigation, which every screen shows.
pub const WELCOME: Tour = Tour {
    id: TourId::Welcome,
    version: 1,
    steps: &[
        TourStep::centered("intro"),
        TourStep::at("strategy", TourAnchor::NavGroup(Pillar::Strategy)),
        TourStep::at("production", TourAnchor::NavGroup(Pillar::Production)),
        TourStep::at("publishing", TourAnchor::NavGroup(Pillar::Publishing)),
        TourStep::at("settings", TourAnchor::NavPlace(Destination::Settings)),
        TourStep::at("jobs", TourAnchor::NavPlace(Destination::Jobs)),
        TourStep::at("costs", TourAnchor::NavPlace(Destination::Costs)),
        TourStep::at("guide", TourAnchor::NavPlace(Destination::Guide)),
    ],
};

impl Tour {
    /// Every tour Bardo ships.
    pub const ALL: [&'static Tour; 1] = [&WELCOME];

    pub fn get(id: TourId) -> &'static Tour {
        match id {
            TourId::Welcome => &WELCOME,
        }
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

/// What lights up for the current step, given what is on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spot {
    Lit(TourAnchor),
    /// The card in the middle, nothing lit.
    Center,
    /// The step's component is missing and the step goes: move on with
    /// [`Bardo::tour_step_over`].
    Skip,
}

/// What the UI does after a move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TourMove {
    /// Show the run's current step, opening its place first when it has
    /// one.
    Show(Option<TourPlace>),
    /// The last step was done: the user stays where the tour left them.
    Finished,
    /// The tour was skipped: back to where the user was when it started.
    Left(Destination),
    /// The tour closed with Esc and can be resumed; nothing moves.
    Closed,
}

/// Why a tour cannot start now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TourError {
    /// The missed posts come first: tours wait until their list closes.
    #[error("the missed posts list is open")]
    MissedPostsOpen,
    #[error("there is no tour to resume")]
    NothingToResume,
}

/// The step on screen, as its card shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TourStepView {
    pub tour: TourId,
    /// From 1.
    pub number: usize,
    pub count: usize,
    pub title: Text,
    pub body: Text,
    pub side: Side,
    pub anchor: Option<TourAnchor>,
}

impl TourStepView {
    pub fn is_first(&self) -> bool {
        self.number == 1
    }

    pub fn is_last(&self) -> bool {
        self.number == self.count
    }
}

/// A tour being shown: its step, where the user was when it started, and
/// which way they were going (a missing step is passed over that way).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TourRun {
    tour: &'static Tour,
    step: usize,
    origin: Destination,
    forward: bool,
}

impl TourRun {
    fn new(tour: &'static Tour, step: usize, origin: Destination) -> Self {
        Self {
            tour,
            step: step.min(tour.len().saturating_sub(1)),
            origin,
            forward: true,
        }
    }

    fn current(&self) -> &'static TourStep {
        &self.tour.steps[self.step]
    }

    fn shown(&self) -> TourMove {
        TourMove::Show(self.current().place)
    }

    /// `None` once past the last step.
    fn next(&mut self) -> Option<TourMove> {
        self.forward = true;
        if self.step + 1 >= self.tour.len() {
            return None;
        }
        self.step += 1;
        Some(self.shown())
    }

    /// Stays on the first step.
    fn back(&mut self) -> TourMove {
        self.forward = false;
        self.step = self.step.saturating_sub(1);
        self.shown()
    }

    /// Passes over a step whose component is missing, the way the user was
    /// going; at the first step it turns forward. `None` once past the
    /// last step.
    fn step_over(&mut self) -> Option<TourMove> {
        if !self.forward && self.step > 0 {
            Some(self.back())
        } else {
            self.next()
        }
    }

    fn spot(&self, on_screen: impl Fn(TourAnchor) -> bool) -> Spot {
        let step = self.current();
        let Some(anchor) = step.anchor else {
            return Spot::Center;
        };
        if on_screen(anchor) {
            return Spot::Lit(anchor);
        }
        match step.when_missing {
            WhenMissing::Skip => Spot::Skip,
            WhenMissing::LightPart(part) if on_screen(part) => Spot::Lit(part),
            WhenMissing::LightPart(_) | WhenMissing::Center => Spot::Center,
        }
    }

    fn view(&self) -> TourStepView {
        let step = self.current();
        TourStepView {
            tour: self.tour.id,
            number: self.step + 1,
            count: self.tour.len(),
            title: Text::TourStepTitle(self.tour.id, step.key),
            body: Text::TourStepBody(self.tour.id, step.key),
            side: step.side,
            anchor: step.anchor,
        }
    }
}

/// The profile's tour progress and the tour on screen.
pub(crate) struct TourBook {
    owner: ProfileId,
    repository: Box<dyn TourProgressRepository>,
    progress: HashMap<TourId, TourProgress>,
    run: Option<TourRun>,
    /// The first-run offer was answered (or a tour started) this session.
    offer_answered: bool,
}

impl TourBook {
    /// Reads the profile's progress. Progress is a convenience: when it
    /// cannot be read, every tour starts as never seen and the failure is
    /// logged, rather than Bardo not opening.
    pub(crate) fn load(owner: ProfileId, repository: Box<dyn TourProgressRepository>) -> Self {
        let progress = match repository.tour_progress(owner) {
            Ok(progress) => progress
                .into_iter()
                .map(|progress| (progress.tour, progress))
                .collect(),
            Err(error) => {
                tracing::warn!(%error, "could not read tour progress");
                HashMap::new()
            }
        };
        Self {
            owner,
            repository,
            progress,
            run: None,
            offer_answered: false,
        }
    }

    /// Remembers `state` at `step` of `tour`. Progress is a convenience:
    /// when it cannot be saved, the tour goes on and the failure is logged.
    fn record(&mut self, tour: &Tour, state: TourState, step: usize) {
        let progress = TourProgress {
            profile: self.owner,
            tour: tour.id,
            version: tour.version,
            state,
            last_step: u32::try_from(step).unwrap_or(u32::MAX),
            updated_at: SystemTime::now(),
        };
        if let Err(error) = self.repository.save_tour_progress(&progress) {
            tracing::warn!(%error, tour = tour.id.code(), "could not save tour progress");
        }
        self.progress.insert(tour.id, progress);
    }

    fn record_run(&mut self) {
        if let Some(run) = self.run {
            self.record(run.tour, TourState::InProgress, run.step);
        }
    }
}

impl Bardo {
    /// Whether to ask "Take a 2-minute tour?": the welcome tour was never
    /// started nor turned down for good, and nothing answered it in this
    /// session.
    pub fn tour_offer(&self) -> bool {
        let book = &self.tours;
        !book.offer_answered
            && book.run.is_none()
            && book
                .progress
                .get(&TourId::Welcome)
                .is_none_or(|progress| progress.state == TourState::Offered)
    }

    /// "Not now" (`never` false): asked again next time Bardo opens.
    /// "Don't show again" (`never` true): never asked again; the tour stays
    /// in the Guide menu.
    pub fn decline_tour_offer(&mut self, never: bool) {
        let book = &mut self.tours;
        book.offer_answered = true;
        let state = if never {
            TourState::Dismissed
        } else {
            TourState::Offered
        };
        book.record(&WELCOME, state, 0);
    }

    /// Starts `tour` from its first step; `from` is where Skip returns.
    /// A tour already on screen closes first, keeping its step.
    pub fn start_tour(
        &mut self,
        tour: TourId,
        from: Destination,
        missed_posts_open: bool,
    ) -> Result<TourMove, TourError> {
        self.begin_tour(Tour::get(tour), 0, from, missed_posts_open)
    }

    /// The tour closed midway, if any: "Resume tour" goes back to it.
    pub fn resumable_tour(&self) -> Option<TourId> {
        let running = self.tours.run.map(|run| run.tour.id);
        TourId::ALL.into_iter().find(|tour| {
            Some(*tour) != running
                && self
                    .tours
                    .progress
                    .get(tour)
                    .is_some_and(|progress| progress.state == TourState::InProgress)
        })
    }

    /// Goes back to the step the closed tour was on. A tour whose content
    /// changed since starts over.
    pub fn resume_tour(
        &mut self,
        from: Destination,
        missed_posts_open: bool,
    ) -> Result<TourMove, TourError> {
        let id = self.resumable_tour().ok_or(TourError::NothingToResume)?;
        let tour = Tour::get(id);
        let step = self
            .tours
            .progress
            .get(&id)
            .filter(|progress| progress.version == tour.version)
            .map_or(0, |progress| progress.last_step as usize);
        self.begin_tour(tour, step, from, missed_posts_open)
    }

    fn begin_tour(
        &mut self,
        tour: &'static Tour,
        step: usize,
        from: Destination,
        missed_posts_open: bool,
    ) -> Result<TourMove, TourError> {
        if missed_posts_open {
            return Err(TourError::MissedPostsOpen);
        }
        let book = &mut self.tours;
        book.record_run();
        book.offer_answered = true;
        let run = TourRun::new(tour, step, from);
        book.run = Some(run);
        book.record_run();
        Ok(run.shown())
    }

    /// The step on screen; `None` without a tour, or while the missed
    /// posts list is open (the tour waits behind it).
    pub fn tour_step(&self, missed_posts_open: bool) -> Option<TourStepView> {
        if missed_posts_open {
            return None;
        }
        self.tours.run.map(|run| run.view())
    }

    /// What lights up for the step on screen, given the anchors on screen.
    pub fn tour_spot(&self, on_screen: impl Fn(TourAnchor) -> bool) -> Option<Spot> {
        self.tours.run.map(|run| run.spot(on_screen))
    }

    /// Next, or Finish on the last step.
    pub fn tour_next(&mut self) -> TourMove {
        let book = &mut self.tours;
        let Some(mut run) = book.run else {
            return TourMove::Closed;
        };
        match run.next() {
            Some(moved) => {
                book.run = Some(run);
                book.record_run();
                moved
            }
            None => self.finish_tour(run),
        }
    }

    pub fn tour_back(&mut self) -> TourMove {
        let book = &mut self.tours;
        let Some(mut run) = book.run else {
            return TourMove::Closed;
        };
        let moved = run.back();
        book.run = Some(run);
        book.record_run();
        moved
    }

    /// Passes over a step whose component is not on screen ([`Spot::Skip`]).
    pub fn tour_step_over(&mut self) -> TourMove {
        let book = &mut self.tours;
        let Some(mut run) = book.run else {
            return TourMove::Closed;
        };
        match run.step_over() {
            Some(moved) => {
                book.run = Some(run);
                book.record_run();
                moved
            }
            None => self.finish_tour(run),
        }
    }

    fn finish_tour(&mut self, run: TourRun) -> TourMove {
        let book = &mut self.tours;
        book.run = None;
        book.record(run.tour, TourState::Completed, run.step);
        TourMove::Finished
    }

    /// Skip: the tour ends for good (no "Resume tour") and the user goes
    /// back to where they were.
    pub fn tour_skip(&mut self) -> TourMove {
        let book = &mut self.tours;
        let Some(run) = book.run.take() else {
            return TourMove::Closed;
        };
        book.record(run.tour, TourState::Dismissed, run.step);
        TourMove::Left(run.origin)
    }

    /// Esc: the tour closes and keeps its step for "Resume tour".
    pub fn tour_close(&mut self) -> TourMove {
        let book = &mut self.tours;
        if let Some(run) = book.run.take() {
            book.record(run.tour, TourState::InProgress, run.step);
        }
        TourMove::Closed
    }

    /// Whether `tour` reads as new: never started (only offered at most),
    /// or its content changed since the user last saw it.
    pub fn tour_is_new(&self, tour: TourId) -> bool {
        self.tours.progress.get(&tour).is_none_or(|progress| {
            progress.state == TourState::Offered || progress.version < Tour::get(tour).version
        })
    }

    /// "Reset tours": every tour reads as never seen, and the welcome offer
    /// comes back at once.
    pub fn reset_tours(&mut self) -> Result<(), AppError> {
        let book = &mut self.tours;
        book.repository.reset_tour_progress(book.owner)?;
        book.progress.clear();
        book.run = None;
        book.offer_answered = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    use bardo_domain::RepositoryError;
    use bardo_storage::Database;

    use super::*;
    use crate::{Catalog, Repositories, testing};

    /// Keeps progress in memory, shared with the test, and can fail.
    #[derive(Clone, Default)]
    struct FakeProgress {
        rows: Rc<RefCell<Vec<TourProgress>>>,
        fail: Rc<RefCell<bool>>,
    }

    impl FakeProgress {
        fn state(&self, tour: TourId) -> Option<(TourState, u32)> {
            self.rows
                .borrow()
                .iter()
                .find(|row| row.tour == tour)
                .map(|row| (row.state, row.last_step))
        }
    }

    impl TourProgressRepository for FakeProgress {
        fn tour_progress(&self, _: ProfileId) -> Result<Vec<TourProgress>, RepositoryError> {
            if *self.fail.borrow() {
                return Err(RepositoryError(std::io::Error::other("unreadable").into()));
            }
            Ok(self.rows.borrow().clone())
        }

        fn save_tour_progress(&self, progress: &TourProgress) -> Result<(), RepositoryError> {
            if *self.fail.borrow() {
                return Err(RepositoryError(std::io::Error::other("disk full").into()));
            }
            let mut rows = self.rows.borrow_mut();
            rows.retain(|row| row.tour != progress.tour);
            rows.push(progress.clone());
            Ok(())
        }

        fn reset_tour_progress(&self, _: ProfileId) -> Result<(), RepositoryError> {
            self.rows.borrow_mut().clear();
            Ok(())
        }
    }

    fn start(progress: &FakeProgress) -> Bardo {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let repositories = Repositories {
            tours: Box::new(progress.clone()),
            ..Repositories::shared(db, Arc::new(bardo_storage::MemorySecretStore::default()))
        };
        Bardo::start(repositories, testing::providers(), Some("en-US")).unwrap()
    }

    fn at(app: &Bardo) -> usize {
        app.tour_step(false).map_or(0, |step| step.number)
    }

    // A tour with every kind of step, for the run's rules.
    const LIT: TourStep = TourStep::at("lit", TourAnchor::Header);
    const PLACED: TourStep = TourStep {
        place: Some(TourPlace::Screen(Destination::Research)),
        ..TourStep::at("placed", TourAnchor::Collection)
    };
    const SKIPPED: TourStep = TourStep {
        when_missing: WhenMissing::Skip,
        ..TourStep::at("skipped", TourAnchor::Toolbar)
    };
    const PART: TourStep = TourStep {
        when_missing: WhenMissing::LightPart(TourAnchor::Inspector),
        ..TourStep::at("part", TourAnchor::NavPlace(Destination::Costs))
    };
    static SAMPLE: Tour = Tour {
        id: TourId::Welcome,
        version: 1,
        steps: &[LIT, SKIPPED, PLACED, PART],
    };

    #[test]
    fn next_walks_the_steps_and_finishes_after_the_last() {
        let mut run = TourRun::new(&SAMPLE, 0, Destination::Projects);
        assert_eq!(run.next(), Some(TourMove::Show(None)));
        assert_eq!(
            run.next(),
            Some(TourMove::Show(Some(TourPlace::Screen(
                Destination::Research
            ))))
        );
        assert_eq!(run.next(), Some(TourMove::Show(None)));
        assert_eq!(run.next(), None);
        assert_eq!(run.step, 3);
    }

    #[test]
    fn back_stays_on_the_first_step() {
        let mut run = TourRun::new(&SAMPLE, 1, Destination::Projects);
        run.back();
        assert_eq!(run.step, 0);
        run.back();
        assert_eq!(run.step, 0);
    }

    #[test]
    fn a_missing_step_is_passed_over_the_way_the_user_was_going() {
        let mut run = TourRun::new(&SAMPLE, 0, Destination::Projects);
        run.next();
        assert_eq!(run.spot(|_| false), Spot::Skip);
        run.step_over();
        assert_eq!(run.step, 2, "forward");

        run.back();
        assert_eq!(run.step, 1);
        run.step_over();
        assert_eq!(run.step, 0, "backward");
    }

    #[test]
    fn a_missing_first_step_turns_forward() {
        static FIRST_MISSING: Tour = Tour {
            id: TourId::Welcome,
            version: 1,
            steps: &[SKIPPED, LIT],
        };
        let mut run = TourRun::new(&FIRST_MISSING, 1, Destination::Projects);
        run.back();
        assert_eq!(run.step_over(), Some(TourMove::Show(None)));
        assert_eq!(run.step, 1);
    }

    #[test]
    fn a_missing_last_step_finishes() {
        static LAST_MISSING: Tour = Tour {
            id: TourId::Welcome,
            version: 1,
            steps: &[LIT, SKIPPED],
        };
        let mut run = TourRun::new(&LAST_MISSING, 1, Destination::Projects);
        assert_eq!(run.step_over(), None);
    }

    #[test]
    fn a_missing_component_lights_its_part_or_centers_the_card() {
        let mut run = TourRun::new(&SAMPLE, 3, Destination::Projects);
        assert_eq!(
            run.spot(|anchor| anchor == TourAnchor::NavPlace(Destination::Costs)),
            Spot::Lit(TourAnchor::NavPlace(Destination::Costs))
        );
        assert_eq!(
            run.spot(|anchor| anchor == TourAnchor::Inspector),
            Spot::Lit(TourAnchor::Inspector)
        );
        assert_eq!(run.spot(|_| false), Spot::Center);

        run.step = 0;
        assert_eq!(run.spot(|_| false), Spot::Center, "the default");
        run.step = 2;
        assert_eq!(run.spot(|_| true), Spot::Lit(TourAnchor::Collection));
    }

    #[test]
    fn the_welcome_tour_has_eight_steps_over_the_navigation() {
        assert_eq!(WELCOME.len(), 8);
        assert_eq!(WELCOME.steps[0].anchor, None, "a welcome in the middle");
        assert_eq!(
            WELCOME.steps[7].anchor,
            Some(TourAnchor::NavPlace(Destination::Guide))
        );
        for step in WELCOME.steps {
            assert_eq!(step.place, None, "{}: opens nothing", step.key);
            assert!(
                matches!(
                    step.anchor,
                    None | Some(TourAnchor::NavGroup(_) | TourAnchor::NavPlace(_))
                ),
                "{}: lights the navigation, on every screen",
                step.key
            );
        }
    }

    #[test]
    fn every_step_has_its_texts_in_every_language() {
        for language in bardo_domain::UiLanguage::ALL {
            let catalog = Catalog::load(language);
            for tour in Tour::ALL {
                for step in tour.steps {
                    for text in [
                        Text::TourStepTitle(tour.id, step.key),
                        Text::TourStepBody(tour.id, step.key),
                    ] {
                        assert!(catalog.has(text), "{language:?}: {text:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn step_keys_are_unique_in_a_tour() {
        for tour in Tour::ALL {
            let mut keys: Vec<_> = tour.steps.iter().map(|step| step.key).collect();
            keys.sort_unstable();
            keys.dedup();
            assert_eq!(keys.len(), tour.len(), "{:?}", tour.id);
        }
    }

    #[test]
    fn the_first_run_offers_the_welcome_tour() {
        let app = start(&FakeProgress::default());
        assert!(app.tour_offer());
        assert!(app.tour_is_new(TourId::Welcome));
        assert_eq!(app.tour_step(false), None);
    }

    #[test]
    fn not_now_asks_again_next_time() {
        let progress = FakeProgress::default();
        let mut app = start(&progress);
        app.decline_tour_offer(false);
        assert!(!app.tour_offer(), "not again in this session");
        assert_eq!(
            progress.state(TourId::Welcome),
            Some((TourState::Offered, 0))
        );

        let app = start(&progress);
        assert!(app.tour_offer());
        assert!(app.tour_is_new(TourId::Welcome), "offered is not seen");
    }

    #[test]
    fn dont_show_again_is_never_asked_again() {
        let progress = FakeProgress::default();
        start(&progress).decline_tour_offer(true);
        let app = start(&progress);
        assert!(!app.tour_offer());
        assert!(!app.tour_is_new(TourId::Welcome), "seen, so not new");
    }

    #[test]
    fn starting_shows_the_first_step_and_ends_the_offer() {
        let progress = FakeProgress::default();
        let mut app = start(&progress);
        let moved = app
            .start_tour(TourId::Welcome, Destination::Projects, false)
            .unwrap();
        assert_eq!(moved, TourMove::Show(None));
        let step = app.tour_step(false).unwrap();
        assert_eq!((step.number, step.count), (1, 8));
        assert!(step.is_first() && !step.is_last());
        assert_eq!(step.title, Text::TourStepTitle(TourId::Welcome, "intro"));
        assert!(!app.tour_offer());
        assert_eq!(
            progress.state(TourId::Welcome),
            Some((TourState::InProgress, 0))
        );
        assert!(!start(&progress).tour_offer(), "started is answered");
    }

    #[test]
    fn next_and_back_move_and_save_the_step() {
        let progress = FakeProgress::default();
        let mut app = start(&progress);
        app.start_tour(TourId::Welcome, Destination::Projects, false)
            .unwrap();
        app.tour_next();
        app.tour_next();
        assert_eq!(at(&app), 3);
        app.tour_back();
        assert_eq!(at(&app), 2);
        assert_eq!(
            progress.state(TourId::Welcome),
            Some((TourState::InProgress, 1))
        );
    }

    #[test]
    fn finish_completes_the_tour_and_stays_put() {
        let progress = FakeProgress::default();
        let mut app = start(&progress);
        app.start_tour(TourId::Welcome, Destination::Costs, false)
            .unwrap();
        for _ in 1..WELCOME.len() {
            assert!(matches!(app.tour_next(), TourMove::Show(_)));
        }
        assert!(app.tour_step(false).unwrap().is_last());
        assert_eq!(app.tour_next(), TourMove::Finished);
        assert_eq!(app.tour_step(false), None);
        assert_eq!(
            progress.state(TourId::Welcome),
            Some((TourState::Completed, 7))
        );
        assert_eq!(app.resumable_tour(), None);
        assert!(!app.tour_is_new(TourId::Welcome));
    }

    #[test]
    fn skip_goes_back_where_the_user_was_and_ends_the_tour() {
        let progress = FakeProgress::default();
        let mut app = start(&progress);
        app.start_tour(TourId::Welcome, Destination::Themes, false)
            .unwrap();
        app.tour_next();
        assert_eq!(app.tour_skip(), TourMove::Left(Destination::Themes));
        assert_eq!(app.tour_step(false), None);
        assert_eq!(app.resumable_tour(), None);
        assert_eq!(
            progress.state(TourId::Welcome),
            Some((TourState::Dismissed, 1))
        );
    }

    #[test]
    fn esc_keeps_the_step_for_resume_tour() {
        let progress = FakeProgress::default();
        let mut app = start(&progress);
        app.start_tour(TourId::Welcome, Destination::Projects, false)
            .unwrap();
        app.tour_next();
        app.tour_next();
        assert_eq!(app.tour_close(), TourMove::Closed);
        assert_eq!(app.tour_step(false), None);
        assert_eq!(app.resumable_tour(), Some(TourId::Welcome));

        // After a restart too.
        let mut app = start(&progress);
        assert!(!app.tour_offer());
        assert_eq!(app.resumable_tour(), Some(TourId::Welcome));
        app.resume_tour(Destination::Personas, false).unwrap();
        assert_eq!(at(&app), 3);
        assert_eq!(app.resumable_tour(), None, "it is on screen");
        assert_eq!(app.tour_skip(), TourMove::Left(Destination::Personas));
    }

    #[test]
    fn nothing_to_resume_without_a_closed_tour() {
        let mut app = start(&FakeProgress::default());
        assert_eq!(
            app.resume_tour(Destination::Projects, false),
            Err(TourError::NothingToResume)
        );
    }

    #[test]
    fn a_tour_whose_content_changed_resumes_from_the_start() {
        let progress = FakeProgress::default();
        progress.rows.borrow_mut().push(TourProgress {
            profile: ProfileId::new(),
            tour: TourId::Welcome,
            version: 0,
            state: TourState::InProgress,
            last_step: 5,
            updated_at: SystemTime::UNIX_EPOCH,
        });
        let mut app = start(&progress);
        assert!(app.tour_is_new(TourId::Welcome), "new content");
        assert!(!app.tour_offer(), "it never restarts on its own");
        app.resume_tour(Destination::Projects, false).unwrap();
        assert_eq!(at(&app), 1);
    }

    #[test]
    fn new_content_shows_a_finished_tour_as_new_without_offering_it() {
        let progress = FakeProgress::default();
        progress.rows.borrow_mut().push(TourProgress {
            profile: ProfileId::new(),
            tour: TourId::Welcome,
            version: 0,
            state: TourState::Completed,
            last_step: 7,
            updated_at: SystemTime::UNIX_EPOCH,
        });
        let app = start(&progress);
        assert!(app.tour_is_new(TourId::Welcome));
        assert!(!app.tour_offer());
        assert_eq!(app.tour_step(false), None);
    }

    #[test]
    fn tours_wait_for_the_missed_posts_list() {
        let mut app = start(&FakeProgress::default());
        assert_eq!(
            app.start_tour(TourId::Welcome, Destination::Projects, true),
            Err(TourError::MissedPostsOpen)
        );
        assert_eq!(app.tour_step(false), None);

        app.start_tour(TourId::Welcome, Destination::Projects, false)
            .unwrap();
        app.tour_next();
        assert_eq!(app.tour_step(true), None, "paused behind the list");
        assert_eq!(at(&app), 2, "and back where it was");
    }

    #[test]
    fn starting_another_tour_keeps_the_running_ones_step() {
        let progress = FakeProgress::default();
        let mut app = start(&progress);
        app.start_tour(TourId::Welcome, Destination::Projects, false)
            .unwrap();
        app.tour_next();
        app.start_tour(TourId::Welcome, Destination::Projects, false)
            .unwrap();
        assert_eq!(at(&app), 1);
    }

    #[test]
    fn a_failed_save_does_not_stop_the_tour() {
        let progress = FakeProgress::default();
        let mut app = start(&progress);
        *progress.fail.borrow_mut() = true;
        app.start_tour(TourId::Welcome, Destination::Projects, false)
            .unwrap();
        app.tour_next();
        assert_eq!(at(&app), 2);
        assert_eq!(progress.state(TourId::Welcome), None);
    }

    #[test]
    fn unreadable_progress_still_opens_bardo() {
        let progress = FakeProgress::default();
        start(&progress).decline_tour_offer(true);
        *progress.fail.borrow_mut() = true;
        let app = start(&progress);
        assert!(app.tour_offer(), "every tour as never seen");
        assert!(app.tour_is_new(TourId::Welcome));
    }

    #[test]
    fn reset_forgets_every_tour_and_offers_again() {
        let progress = FakeProgress::default();
        let mut app = start(&progress);
        app.start_tour(TourId::Welcome, Destination::Projects, false)
            .unwrap();
        app.tour_close();
        app.reset_tours().unwrap();
        assert!(app.tour_offer());
        assert_eq!(app.resumable_tour(), None);
        assert!(app.tour_is_new(TourId::Welcome));
        assert_eq!(progress.state(TourId::Welcome), None);
    }

    #[test]
    fn moves_without_a_tour_do_nothing() {
        let mut app = start(&FakeProgress::default());
        assert_eq!(app.tour_next(), TourMove::Closed);
        assert_eq!(app.tour_back(), TourMove::Closed);
        assert_eq!(app.tour_skip(), TourMove::Closed);
        assert_eq!(app.tour_step_over(), TourMove::Closed);
        assert_eq!(app.tour_spot(|_| true), None);
    }

    #[test]
    fn works_against_real_sqlite() {
        let db = Arc::new(Database::open_in_memory().unwrap());
        let secrets = Arc::new(bardo_storage::MemorySecretStore::default());
        let mut app = Bardo::start(
            Repositories::shared(Arc::clone(&db), secrets.clone()),
            testing::providers(),
            None,
        )
        .unwrap();
        app.start_tour(TourId::Welcome, Destination::Projects, false)
            .unwrap();
        app.tour_next();
        app.tour_close();
        let mut app = Bardo::start(
            Repositories::shared(db, secrets),
            testing::providers(),
            None,
        )
        .unwrap();
        assert!(!app.tour_offer());
        app.resume_tour(Destination::Projects, false).unwrap();
        assert_eq!(at(&app), 2);
    }
}
